#[macro_use] extern crate log;
extern crate env_logger;
extern crate futures;
extern crate tokio;
extern crate tokio_tcp;
extern crate trust_dns_resolver;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use futures::Future;

use tokio::prelude::*;
use tokio::timer::Delay;
use tokio::runtime::current_thread::Runtime;
use tokio_tcp::TcpStream;

use trust_dns_resolver::error::{ResolveError, ResolveResult};
use trust_dns_resolver::lookup_ip::LookupIp;
use trust_dns_resolver::system_conf::read_system_conf;
use trust_dns_resolver::ResolverFuture;
use trust_dns_resolver::config::LookupIpStrategy;

fn main() {
    env_logger::init();
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
    debug!("resolving: {}", host);
    let (conf, mut opts) = read_system_conf()?;
    opts.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    let resolver = ResolverFuture::new(conf, opts);
    Ok(resolver.and_then(move |r| r.lookup_ip(host)))
}

pub fn dial(host: &'static str, port: u16) -> ResolveResult<impl Future<Item = TcpStream, Error = Box<std::error::Error>>> {
    debug!("dialing: {}, {}", host, port);
    let ip_fut = resolve(host)?;
    let res = ip_fut
        .map_err(|e| e.into())
        .and_then(move |ips| {
            let socks = ips.iter().map(move |x| SocketAddr::new(x, port)).collect::<Vec<SocketAddr>>();
            happy_connect(socks.into_iter()).map_err(|e| e.into())
        });
    Ok(Box::new(res))
}

pub fn happy_connect(socks: impl Iterator<Item=SocketAddr>) -> Box<Future<Item = TcpStream, Error = Box<std::error::Error>>> {
    let (tx, rx) = futures::sync::mpsc::channel(1);
    for (i, s) in socks.enumerate() {
        let offset = ((i as u64)+1) * 100;
        let when = Instant::now() + Duration::from_millis(offset);
        let txc = tx.clone();
        debug!("creating delay of {}", offset);
        let delay = Delay::new(when)
            .map_err(|e| panic!("delay errored; err={:?}", e))
            .and_then(move |_| {
                debug!("happy eyeballs start connect: {}", s);
                TcpStream::connect(&s)
                    .map_err(|e| panic!("delay errored; err={:?}", e))
            })
            .and_then(move |c| {
                debug!("happy eyeballs connected: {:?}", c);
                txc.send(c).and_then(|_| Ok(()))
                .map_err(|e| panic!("delay errored; err={:?}", e))
            });
        tokio::spawn(delay);
    }
    Box::new(rx.into_future().map(|x| {
        debug!("happy eyeballs recv: {:?}", x.0);
        x.0.unwrap()
    }).map_err(|e| panic!("delay errored; err={:?}", e)))
}









    //
