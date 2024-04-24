use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::io::{self,BufRead};

// プロセス間通信用のクレート
use interprocess::local_socket::LocalSocketStream;
use std::io::Write;

mod constants;
use constants::CHUNK_SIZE;

mod firfilter;
use firfilter::{create_filter, FilterType};

mod agc;
use agc::{create_agc, AGCType};

// キーボードからの入力コマンドを表すEnum
enum UiCommand {
    AM(i32),
    AGC(i32),
    AF(i32),
    RSSI(String),
    IFOUT(String),
    END,
}

enum InternalCommand {
    AM(FilterType),
    AGC(AGCType),
    None,
    AF(FilterType),
    RSSI(String),
    IFOUT(String),
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
            ["RSSI", param] => param.parse().ok().map(UiCommand::RSSI),
            ["IFOUT", param] => param.parse().ok().map(UiCommand::IFOUT),
            ["END"] => Some(UiCommand::END),
            _ => None,
        }
    }
}


fn main() -> Result<(), anyhow::Error> {

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
                    println!("OK");
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
    let mut rssi_path: String = "".to_string();
    let mut receive_data_path: String = "".to_string();

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
            InternalCommand::RSSI(rssi_name) => {
                if rssi_name != "None" {
                    rssi_path = format!("/tmp/{}", rssi_name);    // /tmpの下に名前付きパイプを作る。
                }
                else {
                    rssi_path = "".to_string();
                }
            },
            InternalCommand::IFOUT(ifout_name) => {
                if ifout_name != "None" {
                    receive_data_path = format!("/tmp/{}", ifout_name);    // /tmpの下に名前付きパイプを作る。
                }
                else {
                    receive_data_path = "".to_string();
                }
            },
            InternalCommand::None => {},
            InternalCommand::END => { break; },
        };

        // データ待ち
        let if_data: [f32; CHUNK_SIZE] = if_rx.recv().unwrap();


        // 中間周波数のデータを名前付きパイプに出力する。
        // 帯域の状態を表示することを想定している。
        if !receive_data_output( &if_data, &receive_data_path ) { receive_data_path = "".to_string(); };

        // IFフィルタ
        let filtered = if_filter(&if_data);

        // AGC
        let (agc_data, rssi) = agc(&filtered);

        // RSSI表示  表示に失敗したらfalseが返る。
        if !rssi_output( rssi, &rssi_path ) { rssi_path = "".to_string(); };

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
        UiCommand::RSSI(param) => {
            InternalCommand::RSSI(param)
        },
        UiCommand::IFOUT(param) => {
            InternalCommand::IFOUT(param)
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

// RSSI データを名前付きパイプに出力する関数
fn rssi_output( rssi: f32, rssi_path: &str ) -> bool {

    let path = rssi_path;

    if path != "" {
        let rssi_pipe = LocalSocketStream::connect(path);
        // RSSIを名前付きパイプに出力する。
        if let Ok(mut rssi_pipe) = rssi_pipe {
            let bytes = rssi.to_ne_bytes();  // RSSIをバイト列に変換する。
            if let Err(e) = rssi_pipe.write_all(&bytes) {
                println!("RSSI用名前付きパイプへの書き込みに失敗しました: {}", e);
                return false;
            }
        } else {
            println!("RSSI用名前付きパイプに出力できません。");
            return false;
        }
    }
    true
}

// 受信データを名前付きパイプに出力する関数
fn receive_data_output( data: &[f32; CHUNK_SIZE], receive_data_path: &str ) -> bool {

    let path = receive_data_path;

    if path != "" {
        let receive_data_pipe = LocalSocketStream::connect(path);
        // 受信データを名前付きパイプに出力する。
        if let Ok(mut receive_data_pipe) = receive_data_pipe {
            // 配列をバイト列Vec<u8>にする。
            let bytes = data.iter().flat_map(|&value| value.to_ne_bytes()).collect::<Vec<u8>>();
            if let Err(e) = receive_data_pipe.write_all(&bytes) {
                println!("受信データ用名前付きパイプへの書き込みに失敗しました: {}", e);
                return false;
            }
        } else {
            println!("受信データ用名前付きパイプに出力できません。");
            return false;
        }
    }
    true
}
