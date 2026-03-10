use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;

use mio::net::{TcpStream as MioTcpStream, UdpSocket as MioUdpSocket};

use spark_transport::async_bridge::contract::{ChannelState, DynChannel};
use spark_transport::evidence::EvidenceSink;
use spark_transport::policy::FlushPolicy;
use spark_transport_contract::suite::{self, KeepAlive};
use spark_transport_mio::TcpChannel;
use spark_transport_mio::{MemDatagramChannel, UdpSocketChannel};

fn make_mem_state(chan_id: u32) -> impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive) {
    move |sink| {
        let io: Box<dyn DynChannel> = Box::new(MemDatagramChannel::new());
        let flush = FlushPolicy::default().budget(64 * 1024);
        (
            ChannelState::new(chan_id, io, 10, 5, flush, sink),
            Box::new(()), // no keepalive needed
        )
    }
}

fn make_tcp_state(chan_id: u32) -> impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive) {
    move |sink| {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap();

        // Client connects and stays alive during the test case.
        let client = TcpStream::connect(addr).expect("connect");
        client.set_nonblocking(true).ok();

        let (srv, _) = listener.accept().expect("accept");
        srv.set_nonblocking(true).expect("nonblocking");
        let mio_srv = MioTcpStream::from_std(srv);

        let ch = TcpChannel::new(chan_id, mio_srv, 1024).expect("tcp channel");
        let io: Box<dyn DynChannel> = Box::new(ch);

        let flush = FlushPolicy::default().budget(64 * 1024);

        (
            ChannelState::new(chan_id, io, 10, 5, flush, sink),
            Box::new(client) as KeepAlive,
        )
    }
}

fn make_udp_state(chan_id: u32) -> impl FnMut(Arc<dyn EvidenceSink>) -> (ChannelState, KeepAlive) {
    move |sink| {
        let a = UdpSocket::bind("127.0.0.1:0").expect("bind a");
        let b = UdpSocket::bind("127.0.0.1:0").expect("bind b");
        a.set_nonblocking(true).expect("nb a");
        b.set_nonblocking(true).expect("nb b");
        a.connect(b.local_addr().unwrap()).expect("connect a");
        b.connect(a.local_addr().unwrap()).expect("connect b");

        let mio_a = MioUdpSocket::from_std(a);

        let ch = UdpSocketChannel::new_connected(chan_id, mio_a);
        let io: Box<dyn DynChannel> = Box::new(ch);

        let flush = FlushPolicy::default().budget(64 * 1024);

        (
            ChannelState::new(chan_id, io, 10, 5, flush, sink),
            Box::new(b) as KeepAlive,
        )
    }
}

#[test]
fn p0_contract_suite_mem() {
    suite::run_p0_suite(make_mem_state(1));
}

#[test]
fn p0_contract_suite_tcp() {
    suite::run_p0_suite(make_tcp_state(2));
}

#[test]
fn p0_contract_suite_udp() {
    suite::run_p0_suite(make_udp_state(3));
}
