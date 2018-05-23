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
    let dialer = Dialer::default();

    let lookup_future = dialer.resolve("www.google.com").unwrap();
    let lookup_res = io_loop.block_on(lookup_future).unwrap();
    println!("resolve: {:?}", lookup_res);

    let dialer = Dialer::default();
    let dial_future = dialer.dial("www.google.com", 80).unwrap();
    let dial_res = io_loop.block_on(dial_future).unwrap();
    println!("dial {:?}", dial_res);
}

/// Dialer uses happy eyeballs to resolve and connect to a host
/// The struct fields are named according to the happy eyeballs
/// rfc [rfc8305](https://tools.ietf.org/html/rfc8305#section-3)
struct Dialer {
    pub resolution_delay: u16,
    pub first_addr_family_count: u8,
    pub conn_attempt_delay: u16,
    pub min_conn_delay: u16,
    pub max_conn_delay: u16,
    pub last_resort_local_synthesis_delay: u16,
}

/// Defaults as specified in the rfc
impl Default for Dialer {
    fn default() -> Self {
        Self {
            resolution_delay: 50, // unused
            first_addr_family_count: 1, // unused
            conn_attempt_delay: 250,
            min_conn_delay: 100, // unused
            max_conn_delay: 2000, // unused
            last_resort_local_synthesis_delay: 2000, // unused
        }
    }
}

    pub fn dial<T: AsRef<Dialer>>(dialer: T, host: &'static str, port: u16) -> ResolveResult<impl Future<Item = TcpStream, Error = Box<std::error::Error>>> {
        debug!("dialing: {}, {}", host, port);
        let ip_fut = dialer.as_ref().resolve(host)?;
        let res = ip_fut
            .map_err(|e| e.into())
            .and_then(move |ips| {
                let socks = ips.iter().map(move |x| SocketAddr::new(x, port)).collect::<Vec<SocketAddr>>();
                dialer.as_ref().happy_connect(socks.into_iter()).map_err(|e| e.into())
            });
        Ok(Box::new(res))
    }

impl Dialer {
    pub fn dial(&self, host: &'static str, port: u16) -> ResolveResult<impl Future<Item = TcpStream, Error = Box<std::error::Error>>> {
        let dialer = self.as_ref();
        debug!("dialing: {}, {}", host, port);
        let ip_fut = self.resolve(host)?;
        let res = ip_fut
            .map_err(|e| e.into())
            .and_then(move |ips| {
                let socks = ips.iter().map(move |x| SocketAddr::new(x, port)).collect::<Vec<SocketAddr>>();
                self.happy_connect(socks.into_iter()).map_err(|e| e.into())
            });
        Ok(Box::new(res))
    }
    pub fn resolve(
        &self,
        host: &'static str,
    ) -> ResolveResult<impl Future<Item = LookupIp, Error = ResolveError>> {
        debug!("resolving: {}", host);
        let (conf, mut opts) = read_system_conf()?;
        opts.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
        let resolver = ResolverFuture::new(conf, opts);
        Ok(resolver.and_then(move |r| r.lookup_ip(host)))
    }
    fn happy_connect(&self, socks: impl Iterator<Item=SocketAddr>) -> Box<Future<Item = TcpStream, Error = Box<std::error::Error>>> {
        let (tx, rx) = futures::sync::mpsc::channel(1);
        for (i, s) in socks.enumerate() {
            let offset = ((i as u64)+1) * self.conn_attempt_delay as u64;
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
}






    //
