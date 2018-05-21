extern crate futures;
extern crate tokio;
extern crate tokio_tcp;
extern crate trust_dns_resolver;
#[macro_use]
extern crate log;

use tokio::net::ConnectFuture;
use std::io;
use std::net::{SocketAddr};

// use futures::task::Context;
// use futures::{Async, , Poll};

use tokio::prelude::*;
use tokio_tcp::TcpStream;
use tokio::runtime::current_thread::Runtime;

use trust_dns_resolver::error::{ResolveError, ResolveResult};
use trust_dns_resolver::lookup_ip::LookupIp;
use trust_dns_resolver::system_conf::read_system_conf;
use trust_dns_resolver::ResolverFuture;
use trust_dns_resolver::config::LookupIpStrategy;

fn main() {
    let mut io_loop = Runtime::new().expect("failed creating a runtime");

    let lookup_future = resolve("www.google.com").unwrap();
    let lookup_res = io_loop.block_on(lookup_future).unwrap();
    println!("resolve: {:?}", lookup_res);

    let dial_future = dial("www.google.com", 80).unwrap();
    let dial_res = io_loop.block_on(dial_future).unwrap();
    println!("dial {:?}", dial_res);
}

pub fn resolve(
    host: &'static str,
) -> ResolveResult<impl Future<Item = LookupIp, Error = ResolveError>> {
    let (conf, mut opts) = read_system_conf()?;
    opts.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    let resolver = ResolverFuture::new(conf, opts);
    Ok(resolver.and_then(move |r| r.lookup_ip(host)))
}

pub fn dial(host: &'static str, port: u16) -> ResolveResult<Box<Future<Item = TcpStream, Error = Box<std::error::Error>>>> {
    let ips = resolve(host)?;
    let res = ips.map_err(|e| e.into()).and_then(move |ips| happy_connect(ips, port).map_err(|e| e.into()));
    Ok(Box::new(res))
}

pub fn happy_connect(ips: LookupIp, port: u16) -> impl Future<Item = TcpStream, Error = impl std::error::Error> {
    let socks = ips.iter().map(|x| SocketAddr::new(x, port));
    Connecting::new(socks)
}

#[derive(Debug)]
struct Connecting {
    addrs: Vec<SocketAddr>,
    conns: Vec<ConnectFuture>
}
impl Connecting {
    pub fn new(addrs: impl Iterator<Item=SocketAddr>) -> Self {
        Connecting{addrs: addrs.collect(), conns: vec![]}
    }
}

impl Future for Connecting {
    type Item = TcpStream;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.addrs.reverse();
        for sock in &mut self.addrs {
            println!("connecting: {:?}", sock);
            self.conns.push(TcpStream::connect(sock));
        }
        self.addrs.clear();
        // TODO: how to race these
        for c in &mut self.conns {
            if let Ok(Async::Ready(x)) = c.poll() {
                return Ok(Async::Ready(x))
            }
        }
        Ok(Async::NotReady)
    }

}
