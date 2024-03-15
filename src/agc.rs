// システムで使う定数の読み込み
use crate::constants::CHUNK_SIZE;


// フィルター関連の定数の定義
pub const N: usize = 3;  // フィルタタップ数。フィルタ次数+1になる。
pub enum AGCType {
    AGC05,
    AGC15,
    AGC20,
    AGC30,
    AGC50,
    None,
}

pub fn create_agc(fselect: AGCType) -> impl FnMut(&[f32]) -> ([f32; CHUNK_SIZE],f32) {

    #[cfg(debug_assertions)]
    debug_agctype_print( &fselect );

    match fselect {
        AGCType::AGC05 => create_agc_filter( AGC05 ),
        AGCType::AGC15 => create_agc_filter( AGC15 ),
        AGCType::AGC20 => create_agc_filter( AGC20 ),
        AGCType::AGC30 => create_agc_filter( AGC30 ),
        AGCType::AGC50 => create_agc_filter( AGC50 ),
        AGCType::None => {
            let none: [f32;N]=[0.0;N];
            create_agc_filter( none )
        },
    }
}

#[cfg(debug_assertions)]
fn debug_agctype_print( fselect: &AGCType) {
    match fselect {
        AGCType::AGC05 => println!("AGC05"),
        AGCType::AGC15 => println!("AGC15"),
        AGCType::AGC20 => println!("AGC20"),
        AGCType::AGC30 => println!("AGC30"),
        AGCType::AGC50 => println!("AGC50"),
        AGCType::None => println!("AGC None"),
    };
}

fn create_agc_filter(coefficients: [f32;N]) -> impl FnMut(&[f32]) -> ([f32; CHUNK_SIZE], f32) {

    let mut filter_state: [f32; N] = [0.0; N];

    move |input: &[f32]| -> ([f32; CHUNK_SIZE],f32) {

        // RSSIを計算する。
        let rssi = input.iter().fold(0.0, |sum, &x| sum + x.abs()) / input.len() as f32;

        filter_state.copy_within(1..N, 0);
        filter_state[N - 1] = rssi;

        // フィルタリング
        let mut filtered_rssi = 0.0;
        for i in 0..N {
            filtered_rssi += filter_state[i] * coefficients[i];
        }

        let mut result_rssi = rssi;
        let mut result: [f32; CHUNK_SIZE]=[0.0; CHUNK_SIZE];
        // RSSIが0.0に近い場合、ゲインが上がりすぎるのを阻止する。
        // AGCがNoneの時はRSSIが0.0になっている。
        if filtered_rssi > 0.01 {
            for (i, &sample) in input.iter().enumerate() {
                result[i] = sample / filtered_rssi * 0.8; // 各要素をrssiで割り、結果を新しい配列に格納
            }
            result_rssi = filtered_rssi;
        }
        else {    // RSSIが0に近い時、データをそのまま出力する。
            for (i, &sample) in input.iter().enumerate() {
                result[i] = sample;  // データをコピーするのみ。
            }
        }
        (result, result_rssi) // データとRSSIのタプルを返す。
    }
}


// ここからフィルター係数の定義

const AGC05: [f32;N] = [0.068947479638086, 0.862105040723828, 0.068947479638086];
const AGC15: [f32;N] = [0.068963512975626, 0.862072974048747, 0.068963512975626];
const AGC20: [f32;N] = [0.068964389839198, 0.862071220321603, 0.068964389839198]; 
const AGC30: [f32;N] = [0.068965016172888, 0.862069967654224, 0.068965016172888]; 
const AGC50: [f32;N] = [0.068965336856565, 0.862069326286871, 0.068965336856565];
