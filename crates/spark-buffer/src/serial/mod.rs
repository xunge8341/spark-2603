//! Serial-line oriented helpers.
//!
//! These utilities are useful for industrial environments (ICS/SCADA, embedded gateways)
//! where protocols often run over UART/RS-485 style links.

pub mod cobs;
pub mod slip;
pub mod hdlc;
