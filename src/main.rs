pub mod ws;
pub mod http;

use std::thread;

use std::io::Read;

fn handle_ws<Connection: std::io::Read + std::io::Write>(mut socket : ws::Websocket<Connection>) {
    while !socket.closed {
        let messages = match socket.read() {
            Err(e) => {
                match e {
                    ws::Error::IoError(e) => {
                        eprintln!("Io Error: {e}");
                        Vec::new()
                    },
                    ws::Error::WebsocketError(ws_error) => {
                        eprintln!("Websocket error: {ws_error}");
                        // TODO: close connection
                        socket.close(Some(1002)).ok();
                        return;
                    },
                }
            },
            Ok(messages) => messages
        };

        for message in &messages {
            match message {
                ws::Message::Binary(binary) => {
                    println!("Received {} bytes", binary.len());
                    socket.send_bytes(&binary).unwrap();
                },
                ws::Message::Text(text) => {
                    println!("Received {} bytes '{}'", text.len(), text);
                    socket.send_text(&text).unwrap();
                },
                ws::Message::Close(code) => {
                    socket.close(*code).ok();
                    break;
                }
            }
        }
    }
}

fn send_file(version: &str, filepath: &str) -> Option<http::ResponseComplete> {
    if filepath.len() == 0 {
        return send_file(version, "index.html");
    }
    let mut f = if let Ok(file) = std::fs::File::open(filepath) {
        file
    }else{
        eprintln!("Could not open file {}", filepath);
        return None;
    };

    let mut data : Vec<u8> = Vec::new();
    if let Err(_) = f.read_to_end(&mut data) {
        return None;
    }

    let extension = if let Some(idx) = filepath.rfind('.') {
        &filepath[idx + 1..]
    }else{
        eprintln!("'{}' has no file extension", filepath);
        return None; // No file extension
    };

    let content_type = match extension {
        "html" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        &_ => {
            eprintln!("Invalid file extension '{extension}'");
            return None;
        }
    };

    let response = http::Response::status(version, "Ok", 200)
        .header("Content-Type", content_type)
        .payload(&data);

    Some(response)
}

fn handle_connection<Connection: std::io::Read + std::io::Write>(mut connection : Connection) {
    let req = match http::parse_request(&mut connection) {
        Ok(req) => req,
        Err(e) => {eprintln!("Could parse request ({e})."); return; }
    };

    if req.get_header("Upgrade") == Some("websocket") {
        if let Some(ws) = ws::upgrade(connection, &req) {
            handle_ws(ws);
            println!("Websocket connection closed");
        }
    }else if req.get_uri().len() > 0 {
        let path = &req.get_uri()[1..];
        if let Some(response) = send_file(req.get_http_version(), path) {
            response.send(&mut connection);
            return;
        }
    }else{
        http::Response::status(req.get_http_version(), "Not Ok", 404)
            .header("Content-Type", "text/html")
            .payload(b"<b>File Not Found: 404</b>")
            .send(&mut connection);
    }
    
}

fn main() {

    let listener = std::net::TcpListener::bind("127.0.0.1:8080").unwrap();

    for res in listener.incoming() {
        if let Ok(connection) = res {
            match connection.peer_addr() {
                Ok(addr) => println!("Accepted connection: {}", addr),
                Err(e) => println!("Accepted connection but could not determine peer address! {}", e)
            };

            connection.set_read_timeout(Some(std::time::Duration::new(1, 0))).unwrap();
            thread::spawn(move || {
                handle_connection(connection);
            });
        }else if let Err(e) = res {
            eprintln!("ERROR: {e}");
        }

    }
}
