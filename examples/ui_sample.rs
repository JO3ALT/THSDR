use interprocess::local_socket::LocalSocketListener;
use std::io::Read;

use crossterm::{
    execute,
    cursor,
    cursor::MoveTo,
    terminal::{self, ClearType},
    style::Print,
};

use std::io::stdout;

fn main() -> std::io::Result<()> {
    let pipe = LocalSocketListener::bind("/tmp/rssi")?;

    _ = execute!(    // 画面消去
        stdout(),
        terminal::Clear(ClearType::All),
    );

    loop {
        match pipe.accept() {
            Ok(mut socket) => {
                let mut buf = [0u8; 4]; // f32は4バイト
                let _bytes_read = socket.read(&mut buf)?;
                let rssi = f32::from_ne_bytes(buf);
                let mut int_rssi = (rssi/1e-10).log(10.0).round() as i32;
                if int_rssi > 11 { int_rssi = 11; }
                if int_rssi < 0 { int_rssi = 0; }
                let rssi_string: String = std::iter::repeat("■").take(int_rssi as usize).collect();
                _ = execute!(
                    stdout(),
                    MoveTo(1, 3),
                    Print("                        "),
                    MoveTo(1, 3),
                    Print(rssi_string),
                );
                println!("\nReceived: {}", rssi );
            },
            Err(e) => println!("正常なデータを受けることができませんでした。: {:?}", e),
        }
    }
}
