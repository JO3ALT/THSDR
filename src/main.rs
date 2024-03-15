use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::io::{self,BufRead};

mod constants;
use constants::CHUNK_SIZE;

mod firfilter;
use firfilter::{create_filter, FilterType};

mod agc;
use agc::{create_agc, AGCType};

use crossterm::{
    execute,
    cursor,
    cursor::MoveTo,
    terminal::{self, ClearType},
    style::Print,
};
use std::io::stdout;

// キーボードからの入力コマンドを表すEnum
enum UiCommand {
    AM(i32),
    AGC(i32),
    AF(i32),
    END,
}

enum InternalCommand {
    AM(FilterType),
    AGC(AGCType),
    None,
    AF(FilterType),
    END,
}

impl UiCommand {
    // 文字列からCommandを生成する関数
    fn from_str(input: &str) -> Option<UiCommand> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["AM", param] => param.parse().ok().map(UiCommand::AM),
            ["AGC", param] => param.parse().ok().map(UiCommand::AGC),
            ["AF", param] => param.parse().ok().map(UiCommand::AF),
            ["END"] => Some(UiCommand::END),
            _ => None,
        }
    }
}


fn main() -> Result<(), anyhow::Error> {
    let mut stdout = stdout();
    _ = execute!(
        stdout,
        terminal::Clear(ClearType::All),
        MoveTo(0, 0),
        Print("Command: "),
    );

    let host = cpal::default_host();

    // チャンネルを作成してキー入力の結果を送信する
    let (tx, rx): (Sender<InternalCommand>,Receiver<InternalCommand>) = channel();

    // IFデータ用のチャンネル
    let( if_tx, if_rx ): (Sender<[f32; CHUNK_SIZE]>,Receiver<[f32; CHUNK_SIZE]>) = channel();

    // 復調後データ用のチャンネル
    let( audio_tx, audio_rx ): (Sender<[f32; CHUNK_SIZE]>,Receiver<[f32; CHUNK_SIZE]>) = channel();

    // 入力側デバイスのオープンと入力ストリームスレッドの起動
    let input_device = host.default_input_device().expect("Failed to get default input device");
    let input_stream = create_input_stream(&input_device, if_tx)?;
    input_stream.play()?;

    // 出力用デバイスのオープンと出力ストリームスレッドの起動
    let output_device = host.default_output_device().expect("Failed to get default output device");
    let output_stream = create_output_stream(&output_device, audio_rx)?;
    output_stream.play()?;

    // UI用スレッドの生成
    let key_input_thread = start_key_input_thread(tx);

    // データ処理用スレッド
    // (tx,rx) チャンネルは、処理の種類を決定するコマンド
    let process_thread = thread::spawn(move || process_thread(if_rx, audio_tx, rx));

    // UI用スレッドの実行と終了待ち
    key_input_thread.join().unwrap();

    // コマンドで終了させる。
    process_thread.join().unwrap();

    Ok(())
}

fn start_key_input_thread(tx: Sender<InternalCommand>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let stdin = io::stdin();
        let handle = stdin.lock();
        for line in handle.lines() {
            let line = line.expect("Failed to read line");

            let internalcomm = match UiCommand::from_str(&line) {
                Some(command) => {
                    // コマンドのデコード
                    println!("OK                        ");
                    command_decode( command )
                }
                None => {
                    InternalCommand::None
                }
            };

            if let InternalCommand::None = internalcomm {
                println!("Invalid command");
            }
            else {
                let mut finished = false;
                // 処理スレッドにコマンドを送信する。
                if let InternalCommand::END = internalcomm {
                    finished = true;
                }
                if tx.send( internalcomm ).is_err() {
                    eprintln!("Error sending key input result to output thread");
                    break;
                }
                if finished {
                    println!("Program finished.");
                    break; // プログラムを終了する
                }
            }
            let mut stdout = stdout();
            _ = execute!(
            stdout,
                MoveTo(9, 0),
                Print("                        "),
                MoveTo(9, 0),
            );
        }
    })
}

fn create_input_stream(
    device: &cpal::Device,
    if_tx: Sender<[f32; CHUNK_SIZE]>,
) -> Result<cpal::Stream, anyhow::Error> {
    let config: cpal::StreamConfig = device.default_input_config()?.into();

    #[cfg(debug_assertions)]
    println!( "Input channels = {}", config.channels );

    let mut if_data: [f32; CHUNK_SIZE] = [0.0; CHUNK_SIZE];
    let mut count = 0;

    let input_stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {

            for i in (0..data.len()).step_by(config.channels as usize) {   // 1チャンネルの信号のみを取り出す
                if_data[count] = data[i];

                count += 1;
                if count >= CHUNK_SIZE {
                    let _ = if_tx.send( if_data ).is_err();
                    count = 0;
                }
            }
        },
        |err| eprintln!("Input error: {:?}", err),
    )?;
    Ok(input_stream)
}

fn create_output_stream(
    device: &cpal::Device,
    audio_rx: Receiver<[f32; CHUNK_SIZE]>,
) -> Result<cpal::Stream, anyhow::Error> {
    let config: cpal::StreamConfig = device.default_output_config()?.into();
    let mut ring_buffer = RingBuffer::new();

    let output_stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {

            // チャンクのサイズとオーディオデータのサイズを合わせる必要がある。
            while ring_buffer.available_data()*(config.channels as usize) < data.len() {
                let audio_data: [f32; CHUNK_SIZE] = audio_rx.recv().unwrap();
                ring_buffer.push_slice( &audio_data );
            }

            for i in (0..data.len()).step_by(config.channels as usize) {
                let d=ring_buffer.pop().unwrap();
                for j in 0..config.channels as usize {
                    data[i+j]= d;
                }
            }
        },
        |err| eprintln!("Output error: {:?}", err),
    )?;
    Ok(output_stream)
}

// データ処理スレッド
fn process_thread( if_rx: Receiver<[f32; CHUNK_SIZE]>, audio_tx: Sender<[f32; CHUNK_SIZE]>, rx: Receiver<InternalCommand> ) {
    let mut if_filter = create_filter(FilterType::AM11K);
    let mut agc = create_agc(AGCType::AGC05);
    let mut af_filter = create_filter(FilterType::AF11K);

    // 受信したデータに対する処理を行う
    loop {
        let command = rx.try_recv().unwrap_or(InternalCommand::None); // キー入力結果を受信
        match command {
            InternalCommand::AM(FilterType::AM3K) => if_filter = create_filter(FilterType::AM3K),
            InternalCommand::AM(FilterType::AM6K) => if_filter = create_filter(FilterType::AM6K),
            InternalCommand::AM(FilterType::AM11K) => if_filter = create_filter(FilterType::AM11K),
            InternalCommand::AM(FilterType::None) => if_filter = create_filter(FilterType::None),
            InternalCommand::AM(_) => if_filter = create_filter(FilterType::None),
            InternalCommand::AGC(AGCType::AGC05) => agc = create_agc(AGCType::AGC05),
            InternalCommand::AGC(AGCType::AGC15) => agc = create_agc(AGCType::AGC15),
            InternalCommand::AGC(AGCType::AGC20) => agc = create_agc(AGCType::AGC20),
            InternalCommand::AGC(AGCType::AGC30) => agc = create_agc(AGCType::AGC30),
            InternalCommand::AGC(AGCType::AGC50) => agc = create_agc(AGCType::AGC50),
            InternalCommand::AGC(AGCType::None) => agc = create_agc(AGCType::None),
            InternalCommand::AF(FilterType::AF3K) => af_filter = create_filter(FilterType::AF3K),
            InternalCommand::AF(FilterType::AF6K) => af_filter = create_filter(FilterType::AF6K),
            InternalCommand::AF(FilterType::AF11K) => af_filter = create_filter(FilterType::AF11K),
            InternalCommand::AF(_) => af_filter = create_filter(FilterType::None),
            InternalCommand::None => {},
            InternalCommand::END => { break; },
        };

        // データ待ち
        let if_data: [f32; CHUNK_SIZE] = if_rx.recv().unwrap();
        // IFフィルタ
        let filtered = if_filter(&if_data);

        // AGC
        let (agc_data, rssi) = agc(&filtered);

        // RSSI表示
        let mut stdout = stdout();

        // カーソル位置を保存する。
        let cursor_position = cursor::position(); // Result型を返す
        let (x, y) = match cursor_position {
            Ok((x, y)) => (x, y),  // 成功した場合、座標を取得
            Err(e) => {
                eprintln!("カーソル位置を取得できませんでした: {}", e);
                (0, 0)  // エラーが発生した場合、デフォルト値を使用
            },
        };
        let mut int_rssi = (rssi/1e-10).log(10.0).round() as i32;
        if int_rssi > 11 { int_rssi = 11; } 
        if int_rssi < 0 { int_rssi = 0; } 
        let rssi_string: String = std::iter::repeat("■").take(int_rssi as usize).collect();
        _ = execute!(
            stdout,
            MoveTo(1, 3),
            Print("                        "),
            MoveTo(1, 3),
            Print(rssi_string),
            MoveTo(x, y),
        );

        // 検波
        let det = agc_data.map( |x| (x.abs()-0.5)*2.0 );

        // AF出力用フィルタ
        let filtered_audio = af_filter( &det );

        // データの送信
        let _ = audio_tx.send( filtered_audio );
    }
}

fn command_decode( comm: UiCommand ) -> InternalCommand {

    match comm {
        UiCommand::AM(param) => {
            InternalCommand::AM(
                match param {
                    i32::MIN..=4 => FilterType::AM3K,
                    5..=8 => FilterType::AM6K,
                    9..=13 => FilterType::AM11K,
                    14..=i32::MAX => FilterType::None,
                })
        },
        UiCommand::AGC(param) => {
            InternalCommand::AGC(
                match param {
                    i32::MIN..=-1 => AGCType::None,
                    0 => AGCType::AGC05,
                    1 => AGCType::AGC15,
                    2 => AGCType::AGC20,
                    3 => AGCType::AGC30,
                    4..=i32::MAX => AGCType::AGC50,
                })
        },
        UiCommand::AF(param) => {
            InternalCommand::AF(
                match param {
                    i32::MIN..=4 => FilterType::AF3K,
                    5..=8 => FilterType::AF6K,
                    9..=i32::MAX => FilterType::AF11K,
                })
        },
        UiCommand::END => {
            InternalCommand::END
        },
    }
}

struct RingBuffer {
    buffer: [f32; 2*CHUNK_SIZE],
    head: usize, // 最新のデータが格納されているインデックス
    tail: usize, // 次に古いデータを上書きするインデックス
    size: usize,
}

impl RingBuffer {
    fn new() -> Self {
        RingBuffer {
            buffer: [0.0; 2*CHUNK_SIZE],
            head: 0,
            tail: 0,
            size: 0,
        }
    }

    fn push_slice(&mut self, values: &[f32]) {
        for &value in values {
            self.buffer[self.tail] = value;
            self.tail = (self.tail + 1) % self.buffer.len();
        }
        self.size += values.len();
    }

    fn pop(&mut self) -> Option<f32> {
        if self.size != 0 {
            let value = self.buffer[self.head];
            self.head = (self.head + 1) % self.buffer.len();
            self.size -= 1;
            Some(value)
        } else {
            None
        }
    }

    fn available_data(&self) -> usize {
        self.size
    }
}
