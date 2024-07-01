use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tonic::Status;

pub struct Event {
    pub resp: oneshot::Sender<Option<Status>>,
    pub payload: Payload,
}

pub enum Payload {
    RegisterTcp {
        port: u16,
        // when the tunneld-client exit, the server will cancel the listener.
        cancel: CancellationToken,
        conn_event_chan: ConnEventChan,
    },
    // RegisterHttp is used to notify the server to register a http tunnel.
    // must provide one of the following fields: port, subdomain, domain.
    RegisterHttp {
        port: Option<u16>,
        subdomain: Option<String>,
        domain: Option<String>,
    },
}

// ConnectionChannel is used to notify the server to add | remove to the connection list.
pub type ConnEventChan = mpsc::Sender<ConnEvent>;

pub enum ConnEvent {
    Add(Conn),
    Remove(String),
}

// this channel has two purposes:
// 1. when the server receives `Start` action from `data` streaming,
// the server will send `DataSender` through this channel,
// 2. when the server receives `Sending` from `data` streaming,
// the server will send `Data` through this channel.
pub type ConnChan = mpsc::Sender<ConnChanDataType>;

pub struct Conn {
    pub id: String,
    pub chan: ConnChan,
    // when the server receives `Close` action from `data` streaming,
    // the server will cancel the connection.
    pub cancel: CancellationToken,
}

pub enum ConnChanDataType {
    DataSender(mpsc::Sender<Vec<u8>>),
    Data(Vec<u8>),
}
