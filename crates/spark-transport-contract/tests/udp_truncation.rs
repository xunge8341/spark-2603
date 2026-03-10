use std::net::UdpSocket;
use std::time::Duration;

use mio::net::UdpSocket as MioUdpSocket;

use spark_transport::io::IoOps;
use spark_transport::KernelError;
use spark_transport_mio::UdpSocketChannel;

/// Contract: UDP recv buffer fill => truncated=true (best-effort) and counter increments.
#[test]
fn udp_truncation_is_observable_best_effort() {
    let a = UdpSocket::bind("127.0.0.1:0").expect("bind a");
    let b = UdpSocket::bind("127.0.0.1:0").expect("bind b");
    a.set_nonblocking(true).expect("nb a");
    b.set_nonblocking(true).expect("nb b");
    a.connect(b.local_addr().unwrap()).expect("connect a");
    b.connect(a.local_addr().unwrap()).expect("connect b");

    // Send a payload larger than receiver buffer.
    let payload = vec![0xABu8; 512];
    a.send(&payload).expect("send");

    let mio_b = MioUdpSocket::from_std(b);
    let mut ch = UdpSocketChannel::new_connected(1, mio_b);

    let mut buf = [0u8; 64];
    // retry loop for nonblocking timing
    for _ in 0..200 {
        match ch.try_read_into(&mut buf) {
            Err(KernelError::WouldBlock) | Err(KernelError::Interrupted) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(e) => panic!("unexpected: {:?}", e),
            Ok(out) => {
                assert!(out.truncated, "expected best-effort truncated flag");
                assert!(ch.rx_truncated_total() >= 1, "expected truncation counter >= 1");
                return;
            }
        }
    }

    panic!("did not receive UDP datagram");
}
