//! A minimal Chrome DevTools Protocol client for Steam's embedded browser. Always `127.0.0.1`,
//! never `localhost`: that resolves to `::1` first, where Steam does not listen.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// A listening loopback port answers within a millisecond; longer only slows down probing.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Also how long a call waits for its reply, whatever arrives meanwhile.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum CdpError {
    /// The endpoint is not there, or the exchange with it failed.
    Link(String),
    /// No reply in time. Unlike a [`Link`](Self::Link) failure, the call may have been carried out.
    Timeout(String),
    /// A script ran and threw. The link itself works.
    Script(String),
}

impl CdpError {
    pub fn new(message: &str) -> Self {
        Self::Link(message.to_string())
    }
}

impl std::fmt::Display for CdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link(message) | Self::Timeout(message) => f.write_str(message),
            Self::Script(message) => write!(f, "script: {message}"),
        }
    }
}

impl std::error::Error for CdpError {}

fn error(context: &str, cause: impl std::fmt::Display) -> CdpError {
    CdpError::Link(format!("{context}: {cause}"))
}

fn no_reply(method: &str) -> CdpError {
    CdpError::Timeout(format!("{method}: no reply in time"))
}

fn open(port: u16) -> Result<TcpStream, CdpError> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|cause| error("debugging port", cause))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|cause| error("socket", cause))?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|cause| error("socket", cause))?;
    Ok(stream)
}

fn targets(port: u16) -> Result<Value, CdpError> {
    let mut stream = open(port)?;
    let request =
        format!("GET /json HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|cause| error("target list", cause))?;

    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    let body = loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|cause| error("target list", cause))?;
        response.extend_from_slice(&chunk[..read]);
        if let Some(body) = complete_body(&response, read == 0) {
            break body;
        }
        if read == 0 {
            return Err(CdpError::new("target list: truncated response"));
        }
    };
    serde_json::from_slice(body).map_err(|cause| error("target list", cause))
}

fn complete_body(response: &[u8], closed: bool) -> Option<&[u8]> {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let (head, body) = (&response[..split], &response[split + 4..]);
    let length = String::from_utf8_lossy(head).lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())?
    });
    match length {
        Some(length) => (body.len() >= length).then(|| &body[..length]),
        None => closed.then_some(body),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SteamUi {
    /// Websocket URL of the shared JavaScript context; it exists from early in Steam's start-up.
    pub shared_context: Option<String>,
    pub big_picture: bool,
}

pub fn steam_ui(port: u16) -> Result<SteamUi, CdpError> {
    let targets = targets(port)?;
    let mut big_picture = false;
    let mut shared = None;
    for target in targets.as_array().into_iter().flatten() {
        let title = target["title"].as_str().unwrap_or_default();
        let url = target["url"].as_str().unwrap_or_default();
        // The title is localised ("Big-Picture-Modus"); the user agent marker is not.
        if url.contains("useragent=Valve%20Steam%20Gamepad")
            || (title.starts_with("Big") && title.contains("Picture"))
        {
            big_picture = true;
        }
        if title == "SharedJSContext" {
            shared = target["webSocketDebuggerUrl"]
                .as_str()
                .map(|url| url.replace("://localhost:", "://127.0.0.1:"));
        }
    }
    Ok(SteamUi {
        shared_context: shared,
        big_picture,
    })
}

/// Scripts registered through a session live exactly as long as it stays open.
pub struct Session {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl Session {
    pub fn connect(port: u16, url: &str) -> Result<Self, CdpError> {
        let stream = open(port)?;
        let (socket, _response) =
            tungstenite::client::client(url, stream).map_err(|cause| error("websocket", cause))?;
        Ok(Self { socket, next_id: 0 })
    }

    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, CdpError> {
        self.call_until(method, params, Instant::now() + IO_TIMEOUT)
    }

    /// Waits for the reply until `deadline`, and skips whatever else arrives meanwhile, however
    /// much: a session that stayed open while idle may have a backlog of events.
    fn call_until(
        &mut self,
        method: &str,
        params: &Value,
        deadline: Instant,
    ) -> Result<Value, CdpError> {
        self.next_id += 1;
        let id = self.next_id;
        let request = json!({ "id": id, "method": method, "params": params }).to_string();
        self.socket
            .send(Message::text(request))
            .map_err(|cause| error(method, cause))?;

        loop {
            // Zero is not a timeout the socket takes; it means the time is up.
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(no_reply(method));
            }
            self.socket
                .get_ref()
                .set_read_timeout(Some(left))
                .map_err(|cause| error("socket", cause))?;
            let message = self.socket.read().map_err(|cause| match cause {
                // How a read reports its time limit depends on the platform: Windows may say
                // TimedOut, Unix WouldBlock (`TcpStream::set_read_timeout`).
                tungstenite::Error::Io(io)
                    if matches!(io.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    no_reply(method)
                }
                cause => error(method, cause),
            })?;
            if !message.is_text() {
                continue;
            }
            let text = message.into_text().map_err(|cause| error(method, cause))?;
            let Ok(reply) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            if reply["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(failure) = reply.get("error") {
                return Err(error(method, failure));
            }
            return Ok(reply["result"].clone());
        }
    }

    /// A script that throws is an error, although the protocol answers it like any other.
    pub fn evaluate(&mut self, expression: &str) -> Result<Value, CdpError> {
        let result = self.call(
            "Runtime.evaluate",
            &json!({ "expression": expression, "returnByValue": true }),
        )?;
        evaluated(result)
    }
}

/// The value in a `Runtime.evaluate` result, or what the script threw. Any other shape is null.
fn evaluated(mut result: Value) -> Result<Value, CdpError> {
    let Some(details) = result.get("exceptionDetails") else {
        // Not `result["result"]["value"]`: taking it needs the mutable index, which panics on
        // anything but an object or null, and a panic ends the agent.
        return Ok(result
            .get_mut("result")
            .and_then(|object| object.get_mut("value"))
            .map_or(Value::Null, Value::take));
    };
    // `text` alone is mostly just "Uncaught". An Error's description starts with its type and
    // message, followed by the stack; anything else thrown has a value instead.
    let text = details["text"].as_str().unwrap_or("exception");
    let exception = &details["exception"];
    let thrown = match exception["description"].as_str() {
        Some(description) => description.lines().next().map(str::to_string),
        None => exception.get("value").map(Value::to_string),
    };
    Err(CdpError::Script(match thrown {
        Some(thrown) => format!("{text} {thrown}"),
        None => text.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    /// A websocket on a free loopback port that `serve` answers; returns the port.
    fn fake_endpoint(serve: impl FnOnce(&mut WebSocket<TcpStream>) + Send + 'static) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            serve(&mut socket);
        });
        port
    }

    fn connect(port: u16) -> Session {
        Session::connect(port, &format!("ws://127.0.0.1:{port}/devtools/page/ABC")).unwrap()
    }

    fn request_id(socket: &mut WebSocket<TcpStream>) -> Value {
        let text = socket.read().unwrap().into_text().unwrap();
        serde_json::from_str::<Value>(text.as_str()).unwrap()["id"].clone()
    }

    #[test]
    fn a_script_that_throws_is_an_error() {
        let thrown = json!({
            "result": { "type": "object", "subtype": "error",
                        "description": "TypeError: x is not a function\n    at <anonymous>:1:1" },
            "exceptionDetails": {
                "exceptionId": 1, "text": "Uncaught", "lineNumber": 0, "columnNumber": 0,
                "exception": { "type": "object", "subtype": "error",
                               "description": "TypeError: x is not a function\n    at <anonymous>:1:1" }
            }
        });
        let error = evaluated(thrown).unwrap_err();
        assert!(matches!(error, CdpError::Script(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            "script: Uncaught TypeError: x is not a function"
        );

        let thrown_string = json!({
            "result": { "type": "string", "value": "boom" },
            "exceptionDetails": { "exceptionId": 2, "text": "Uncaught",
                                  "exception": { "type": "string", "value": "boom" } }
        });
        assert_eq!(
            evaluated(thrown_string).unwrap_err().to_string(),
            r#"script: Uncaught "boom""#
        );

        let bare = json!({ "exceptionDetails": { "text": "SyntaxError" } });
        assert_eq!(
            evaluated(bare).unwrap_err().to_string(),
            "script: SyntaxError"
        );
    }

    #[test]
    fn a_value_is_returned_as_it_is() {
        let answer = json!({ "result": { "type": "string", "value": "ok" } });
        assert_eq!(evaluated(answer).unwrap(), json!("ok"));
        let nothing = json!({ "result": { "type": "undefined" } });
        assert_eq!(evaluated(nothing).unwrap(), Value::Null);
    }

    #[test]
    fn a_reply_of_the_wrong_shape_has_no_value() {
        for reply in [
            json!("x"),
            json!(7),
            json!({ "result": "x" }),
            json!({ "result": 5 }),
        ] {
            assert_eq!(evaluated(reply.clone()).unwrap(), Value::Null, "{reply}");
        }
    }

    #[test]
    fn a_reply_is_found_behind_any_number_of_events() {
        let port = fake_endpoint(|socket| {
            let id = request_id(socket);
            for _ in 0..500 {
                let event = json!({ "method": "Page.frameNavigated", "params": {} });
                socket.send(Message::text(event.to_string())).unwrap();
            }
            let reply =
                json!({ "id": id, "result": { "result": { "type": "number", "value": 2 } } });
            socket.send(Message::text(reply.to_string())).unwrap();
            let _ = socket.read();
        });
        assert_eq!(connect(port).evaluate("1 + 1").unwrap(), json!(2));
    }

    #[test]
    fn a_call_without_a_reply_ends_at_its_deadline() {
        let port = fake_endpoint(|socket| {
            request_id(socket);
            let event = json!({ "method": "Page.frameNavigated", "params": {} });
            socket.send(Message::text(event.to_string())).unwrap();
            // Keeps the connection open, and silent, until the client gives up.
            let _ = socket.read();
        });
        let mut session = connect(port);
        let started = Instant::now();
        let deadline = started + Duration::from_millis(300);
        let error = session
            .call_until("Runtime.evaluate", &json!({}), deadline)
            .unwrap_err();
        assert!(matches!(error, CdpError::Timeout(_)), "{error:?}");
        // At the deadline, not at the socket's own timeout; the rest is room for a slow runner.
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(250) && waited < Duration::from_secs(2),
            "{waited:?}"
        );
    }

    #[test]
    fn a_session_closed_at_the_other_end_is_a_broken_link() {
        let port = fake_endpoint(|_| {});
        let mut session = connect(port);
        let started = Instant::now();
        let error = session.evaluate("1 + 1").unwrap_err();
        assert!(matches!(error, CdpError::Link(_)), "{error:?}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn body_is_complete_by_length_or_by_close() {
        let with_length = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        assert_eq!(complete_body(with_length, false), Some(&b"[]"[..]));
        assert_eq!(
            complete_body(&with_length[..with_length.len() - 1], false),
            None
        );

        let without_length = b"HTTP/1.1 200 OK\r\n\r\n[]";
        assert_eq!(complete_body(without_length, false), None);
        assert_eq!(complete_body(without_length, true), Some(&b"[]"[..]));
    }
}
