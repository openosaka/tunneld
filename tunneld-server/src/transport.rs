use std::sync::Arc;

use http_body_util::Full;
use bytes::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use anyhow::Context as _;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use tokio::io::AsyncWriteExt; // for shutdown() method
use tokio::{io, select, spawn, sync::mpsc};
use tokio_util::sync::CancellationToken;
use tonic::Status;
use tracing::debug;
use std::convert::Infallible;
use hyper::{Request, Response};
use tunneld_pkg::{
    event,
    io::{StreamingReader, StreamingWriter, VecWrapper},
    util::create_listener,
};
use uuid::Uuid;

pub struct EventBus {
    vhttp_port: u16,
    domain: String,
}

impl EventBus {
    pub fn new(vhttp_port: u16, domain: String) -> Self {
        Self {
            vhttp_port,
            domain,
        }
    }

    pub async fn listen(self, mut receiver: mpsc::Receiver<event::Event>) {
        let this = Arc::new(self);

        while let Some(event) = receiver.recv().await {
            match event.payload {
                event::Payload::RegisterTcp {
                    port,
                    cancel,
                    conn_event_chan,
                } => match create_listener(port).await {
                    Ok(listener) => {
                        let this = Arc::clone(&this);
                        spawn(async move {
                            this.handle_listener(listener, cancel, conn_event_chan.clone())
                                .await;
                            debug!("tcp listener on {} closed", port);
                        });
                        event.resp.send(None).unwrap(); // success
                    }
                    Err(status) => {
                        event.resp.send(Some(status)).unwrap();
                    }
                },
                event::Payload::RegisterHttp {
                    port,
                    subdomain,
                    domain,
                } => {
                    if port.is_none()
                        && subdomain.is_none()
                        && domain.is_none()
                    {
                        event.resp.send(Some(Status::invalid_argument("invalid http tunnel arguments"))).unwrap();
                        continue;
                    }
                }
            }
        }

        debug!("tcp manager quit");
    }

    async fn handle_listener(
        &self,
        listener: tokio::net::TcpListener,
        shutdown: CancellationToken,
        conn_event_chan: mpsc::Sender<event::ConnEvent>,
    ) {
        loop {
            select! {
                _ = shutdown.cancelled() => {
                    return;
                }
                result = async {
                    match listener.accept().await  {
                        Ok(result) => {
                            Some(result)
                        }
                        Err(err) => {
                            debug!("failed to accept connection: {:?}", err);
                            None
                        }
                    }
                } => {
                    if result.is_none() {
                        return;
                    }
                    let (stream, _addr) = result.unwrap();
                    let connection_id = Uuid::new_v4().to_string();
                    let (data_channel, mut data_channel_rx) = mpsc::channel(1024);

                    let cancel_w = CancellationToken::new();
                    let cancel = cancel_w.clone();

                    let event = event::Conn{
                        id: connection_id.clone(),
                        chan: data_channel.clone(),
                        cancel: cancel_w,
                    };
                    conn_event_chan.send(event::ConnEvent::Add(event)).await.unwrap();
                    let data_sender = data_channel_rx.recv().await.context("failed to receive data_sender").unwrap();
                    let data_sender = {
                        match data_sender {
                            event::ConnChanDataType::DataSender(sender) => sender,
                            _ => panic!("we expect to receive DataSender from data_channel_rx at the first time."),
                        }
                    };

                    let conn_event_chan_for_removing = conn_event_chan.clone();
                    tokio::spawn(async move {
                        let (mut remote_reader, mut remote_writer) = stream.into_split();
                        let wrapper = VecWrapper::<Vec<u8>>::new();
                        let mut tunnel_writer = StreamingWriter::new(data_sender, wrapper);
                        let mut tunnel_reader = StreamingReader::new(data_channel_rx); // we expect to receive data from data_channel_rx after the first time.
                        let remote_to_me_to_tunnel = async {
                            io::copy(&mut remote_reader, &mut tunnel_writer).await.unwrap();
                            tunnel_writer.shutdown().await.context("failed to shutdown tunnel writer").unwrap();
                            debug!("finished the transfer between remote and tunnel");
                        };
                        let tunnel_to_me_to_remote = async {
                            io::copy(&mut tunnel_reader, &mut remote_writer).await.unwrap();
                            remote_writer.shutdown().await.context("failed to shutdown remote writer").unwrap();
                            debug!("finished the transfer between tunnel and remote");
                        };

                        tokio::select! {
                            _ = async { tokio::join!(remote_to_me_to_tunnel, tunnel_to_me_to_remote) } => {
                                debug!("closing user connection {}", connection_id);
                                conn_event_chan_for_removing
                                    .send(event::ConnEvent::Remove(connection_id))
                                    .await
                                    .context("notify server to remove connection channel")
                                    .unwrap();
                            }
                            _ = cancel.cancelled() => {
                                let _ = remote_writer.shutdown().await;
                                let _ = tunnel_writer.shutdown().await;
                            }
                        }
                    });
                }
            }
        }
    }
}

pub struct HttpManager {
    pub port: u16,
    pub domain: String,
}

impl HttpManager {
    pub fn new(port: u16, domain: String) -> Self {
        Self { port, domain }
    }

    pub async fn run(self) {
        let listener = create_listener(self.port).await.unwrap();
        let this = Arc::new(self);

        while let Ok((stream, _addr)) = listener.accept().await {
            let this = Arc::clone(&this);
            tokio::spawn(async move {
                this.handle_connection(stream).await;
            });
        }
    }

    async fn handle_connection(&self, stream: tokio::net::TcpStream) {
        let io = TokioIo::new(stream);
        let _handler = tokio::task::spawn(async move {
            Builder::new(TokioExecutor::new())
                .http1()
                .serve_connection(io, service_fn(http_tunnel))
                .await
        });
    }
}

async fn http_tunnel(
    _req: Request<impl hyper::body::Body>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello World!"))))
}