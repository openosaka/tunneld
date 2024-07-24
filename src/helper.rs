use crate::pb::RegisterReq;

use tokio::net::{TcpListener, UdpSocket};
use tonic::Status;
use tracing::error;

pub(crate) async fn create_tcp_listener(port: u16) -> Result<TcpListener, Status> {
    TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(map_bind_error)
}

pub(crate) async fn create_udp_socket(port: u16) -> Result<UdpSocket, Status> {
    UdpSocket::bind(("0.0.0.0", port))
        .await
        .map_err(map_bind_error)
}

fn map_bind_error(err: std::io::Error) -> Status {
    match err.kind() {
        std::io::ErrorKind::AddrInUse => Status::already_exists("port already in use"),
        std::io::ErrorKind::PermissionDenied => Status::permission_denied("permission denied"),
        _ => {
            error!("failed to bind port: {}", err);
            Status::internal("failed to bind port")
        }
    }
}

pub fn validate_register_req(req: &RegisterReq) -> Option<Status> {
    if req.tunnel.is_none() {
        return Some(Status::invalid_argument("tunnel is required"));
    }
    let tunnel = req.tunnel.as_ref().unwrap();
    if tunnel.config.is_none() {
        return Some(Status::invalid_argument("config is required"));
    }
    None
}
