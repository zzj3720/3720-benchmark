use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

#[test]
fn status_reaches_the_configured_sidecar() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut chunk = [0_u8; 256];
            let length = stream.read(&mut chunk).unwrap();
            assert_ne!(length, 0, "client closed before sending HTTP headers");
            request.extend_from_slice(&chunk[..length]);
        }
        assert!(String::from_utf8_lossy(&request).starts_with("GET /v1/status "));
        let body = br#"{"ok":true,"data":{"tick":0}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_swarm"))
        .arg("status")
        .env("SWARM_URL", format!("http://{address}"))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{\"ok\":true,\"data\":{\"tick\":0}}");
}
