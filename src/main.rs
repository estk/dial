extern crate futures;
extern crate tokio;
extern crate tokio_tcp;
extern crate trust_dns_resolver;
#[macro_use] extern crate log;

use std::net::*;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs};
use std::time::{Duration, SystemTime};
use std::vec;

// use futures::task::Context;
// use futures::{Async, , Poll};

use tokio::runtime::current_thread::Runtime;
use tokio::prelude::*;

use trust_dns_resolver::ResolverFuture;
use trust_dns_resolver::lookup_ip::{LookupIpFuture, LookupIp};
use trust_dns_resolver::error::{ResolveResult, ResolveError};


fn main() {
    let lookup_future = resolve("www.reddit.com").unwrap();
    let mut io_loop = Runtime::new().expect("failed creating a runtime");
    let lookup_res = io_loop.block_on(lookup_future).unwrap();
    println!("resolve: {:?}", lookup_res);
}

pub fn resolve(host: &'static str) -> ResolveResult<Box<Future<Item=LookupIp, Error=ResolveError>>> {
    let resolver = ResolverFuture::from_system_conf()?;
    Ok(Box::new(resolver.and_then(move |r| r.lookup_ip(host))))
}

// #[derive(Debug)]
// struct Connecting {
//     addrs: vec::IntoIter<SocketAddr>,
//     conns: Vec<ConnectFuture>
// }
// impl Connecting {
//     pub fn new(addrs: vec::IntoIter<SocketAddr>) -> Self {
//         Connecting{addrs: addrs, conns: vec![]}
//     }
// }
//
// impl Future for Connecting {
//     type Item = TcpStream;
//     type Error = io::Error;
//     fn poll(&mut self, cx: &mut Context) -> Poll<Self::Item, Self::Error> {
//         // Todo initiate connections
//         unimplemented!()
//     }
//
// }
