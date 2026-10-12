//! A clean-room MPEG-1 Layer III (MP3) encoder, written from ISO/IEC 11172-3 (upstream gap G-21).
//!
//! Work in progress, built in reviewable increments; [`crate::encode`] does not offer MP3 yet.
//! Done: the analysis filterbank (polyphase filterbank, MDCT, alias reduction). Next: quantization
//! and Huffman coding of the MDCT lines, side information and frame packing for CBR at 32, 44.1
//! and 48 kHz, then the bit reservoir and an Info tag for gapless playback.
// Nothing outside the tests calls the encoder until the bitstream writer lands.
#![cfg_attr(not(test), allow(dead_code))]

mod filterbank;
mod tables;
