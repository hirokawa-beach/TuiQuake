use std::{
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

use chrono::DateTime;
use serde_json::Value;

use super::{Result, ServerClock};

pub(super) type LiveSocket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

pub(super) fn connect_live(url: &str) -> Result<(LiveSocket, Option<ServerClock>)> {
    use tungstenite::{
        client::IntoClientRequest, client_tls_with_config, http::HeaderValue,
        protocol::WebSocketConfig, stream::MaybeTlsStream,
    };

    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        "User-Agent",
        HeaderValue::from_static(concat!("TuiQuake/", env!("CARGO_PKG_VERSION"))),
    );
    let host = request.uri().host().ok_or("WebSocket のホストが不明です")?;
    let timeout = Duration::from_secs(7);
    let mut last_error = None;
    let tcp = (host, 443)
        .to_socket_addrs()?
        .find_map(|addr| match TcpStream::connect_timeout(&addr, timeout) {
            Ok(socket) => Some(socket),
            Err(error) => {
                last_error = Some(std::io::Error::new(
                    error.kind(),
                    format!("{addr}: {error}"),
                ));
                None
            }
        })
        .ok_or_else(|| {
            last_error.unwrap_or_else(|| std::io::Error::other("WebSocket の接続先がありません"))
        })?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    let (mut socket, response) = client_tls_with_config(request, tcp, Some(config), None)?;
    let clock = http_clock(response.headers(), Instant::now());
    let MaybeTlsStream::Rustls(stream) = socket.get_mut() else {
        return Err("WebSocket が TLS を使用していません".into());
    };
    stream
        .sock
        .set_read_timeout(Some(Duration::from_millis(300)))?;
    Ok((socket, clock))
}

pub(super) fn http_clock(
    headers: &tungstenite::http::HeaderMap,
    received: Instant,
) -> Option<ServerClock> {
    let date = headers.get("date")?.to_str().ok()?;
    let utc = DateTime::parse_from_rfc2822(date).ok()?.to_utc();
    let age = headers
        .get("age")
        .and_then(|v| v.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    let utc = utc.checked_add_signed(chrono::TimeDelta::try_seconds(age)?)?;
    Some(ServerClock { utc, received })
}

pub(super) fn read_live(socket: &mut LiveSocket) -> tungstenite::Result<Option<Value>> {
    use tungstenite::{Error, Message};
    match socket.read()? {
        Message::Text(message) => {
            serde_json::from_str(message.as_str())
                .map(Some)
                .map_err(|error| {
                    Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
                })
        }
        Message::Ping(_) => {
            socket.flush()?;
            Ok(None)
        }
        Message::Close(_) => {
            let _ = socket.flush();
            Err(Error::ConnectionClosed)
        }
        _ => Ok(None),
    }
}

pub(super) fn read_timed_out(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(error)
        if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock))
}
