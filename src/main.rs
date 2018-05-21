extern crate futures;
#[macro_use]
extern crate log;
extern crate tokio_tcp;

use tokio_tcp::{TcpStream, ConnectFuture};

use futures::executor::ThreadPool;
use futures::task::Context;
use futures::{Async, Future, Poll};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs};
use std::time::{Duration, SystemTime};
use std::vec;

fn main() {
    // let now = SystemTime::now();
    let fut = Resolve::new("aorsthn.com".to_string(), 80);
    let res = ThreadPool::new()
        .expect("Failed to create threadpool")
        .run(fut);
    // println!("duration: {:?}", now.elapsed().expect("system clock should work").subsec_millis());
    println!("resolve: {:?}", res);
}

fn new_resolver() {
    let resolver_fut = ResolverFuture::from_system_conf();
    let response = resolver.lookup_ip.unwrap();
    resolver_fut.then(|r| r.lookup_ip("www.example.com."))
}

pub fn resolve(host: &str) -> Future<Iterator<IpAddr>>{
    ResolverFuture::from_system_conf()
        .then(|r| r.lookup_ip("www.example.com."))
}

#[derive(Debug)]
struct Resolve {
    host: String,
    port: u16,
}
impl Resolve {
    pub fn new(host: String, port: u16) -> Self {
        Resolve { host, port }
    }
}

impl Future for Resolve {
    type Item = Connecting;
    type Error = io::Error;

    fn poll(&mut self, _cx: &mut Context) -> Poll<Self::Item, Self::Error> {
        debug!("resolving host={:?}, port={:?}", self.host, self.port);
        (&*self.host, self.port)
            .to_socket_addrs()
            .map(|i| Async::Ready(Connecting::new(i)))
    }
}

#[derive(Debug)]
struct Connecting {
    addrs: vec::IntoIter<SocketAddr>,
    conns: Vec<ConnectFuture>
}
impl Connecting {
    pub fn new(addrs: vec::IntoIter<SocketAddr>) -> Self {
        Connecting{addrs: addrs, conns: vec![]}
    }
}

impl Future for Connecting {
    type Item = TcpStream;
    type Error = io::Error;
    fn poll(&mut self, cx: &mut Context) -> Poll<Self::Item, Self::Error> {
        // Todo initiate connections
        unimplemented!()
    }

}
