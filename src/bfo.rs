use crate::constants::{CHUNK_SIZE, SAMPLING_FREQ};
use core::f32::consts::PI;

pub fn create_bfo() -> impl FnMut(f32) -> [f32; CHUNK_SIZE] {

    let mut phase:f32 = 0.0;

    move |freq: f32| -> [f32; CHUNK_SIZE] {
        let mut result: [f32; CHUNK_SIZE]=[0.0; CHUNK_SIZE];

        let phase_delta = 2.0*PI*freq/SAMPLING_FREQ;

        for i in 0..CHUNK_SIZE {
            result[i] = phase.sin();
            phase = phase+phase_delta;
            if phase > 2.0*PI {
                phase -= 2.0*PI;
            }
        }
        result
    }
}
