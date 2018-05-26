#[macro_use]
extern crate log;
extern crate env_logger;
extern crate futures;
extern crate tokio;
extern crate tokio_tcp;
extern crate trust_dns_resolver;

use futures::sync::oneshot;
use std::net::IpAddr;
use trust_dns_resolver::lookup_ip::LookupIp;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::prelude::stream::futures_unordered;

use futures::Future;

use tokio::prelude::*;
use tokio::runtime::current_thread::Runtime;
use tokio::timer::Delay;
use tokio_tcp::TcpStream;

use trust_dns_resolver::config::LookupIpStrategy;
use trust_dns_resolver::error::{ResolveError, ResolveResult};
use trust_dns_resolver::system_conf::read_system_conf;
use trust_dns_resolver::ResolverFuture;

fn main() {
    env_logger::init();
    let mut io_loop = Runtime::new().expect("failed creating a runtime");
    // let lookup_future = dialer.resolve("www.google.com").unwrap();
    // let lookup_res = io_loop.block_on(lookup_future).unwrap();
    // println!("resolve: {:?}", lookup_res);

    let dialer = Dialer::default();
    let dial_future = dialer.dial("www.google.com", 80).unwrap();
    let dial_res = io_loop.block_on(dial_future).unwrap();
    println!("dial {:?}", dial_res);
}

pub fn dial(
    host: &'static str,
    port: u16,
) -> ResolveResult<impl Future<Item = TcpStream, Error = Box<std::error::Error>>> {
    Dialer::default().dial(host, port)
}

/// Dialer uses happy eyeballs to resolve and connect to a host
/// The struct fields are named according to the happy eyeballs
/// rfc [rfc8305](https://tools.ietf.org/html/rfc8305#section-3)
#[derive(Debug, Clone, Copy)]
struct Dialer {
    pub prefer_v6: bool,
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
            prefer_v6: true,
            resolution_delay: 50,       // unused
            first_addr_family_count: 1, // unused
            conn_attempt_delay: 250,
            min_conn_delay: 100,                     // unused
            max_conn_delay: 2000,                    // unused
            last_resort_local_synthesis_delay: 2000, // unused
        }
    }
}

impl Dialer {
    pub fn dial(
        self,
        host: &'static str,
        port: u16,
    ) -> ResolveResult<impl Future<Item = TcpStream, Error = Box<std::error::Error>>> {
        debug!("dialing: {}, {}", host, port);

        // resolve should be a stream
        let (v4, v6) = self.resolve(host)?;
        let ordered = self.order(v4, v6);
        let socks = ordered
            .map(|ip| SocketAddr::new(ip, port));

        Ok(Box::new(self.connect(socks)))
    }
    fn order(
        &self,
        v4: impl Future<Item = LookupIp, Error = ResolveError> + Send,
        v6: impl Future<Item = LookupIp, Error = ResolveError> + Send,
    ) -> impl Stream<Item = IpAddr, Error = ()> {
        let (tx, rx) = futures::sync::mpsc::channel(1);
        let (unblock_v4, blocked_v4) = oneshot::channel();
        let (unblock_v6, blocked_v6) = oneshot::channel();
        if self.prefer_v6 {
            unblock_v6.send(());
        } else {
            unblock_v4.send(());
        }
        tokio::spawn(v4
            .map_err(|e| eprintln!("accept failed = {:?}", e))
            .and_then(|lip| {
                unblock_v6.send(());
                blocked_v4.and_then(|_| {
                    stream::iter_ok(lip.iter()).and_then(|ip| {
                        tx.send(ip)
                })
                .into()
            })
        }));
        tokio::spawn(v6
            .map_err(|e| eprintln!("accept failed = {:?}", e))
            .and_then(|lip| {
                unblock_v4.send(());
                blocked_v6.and_then(|_| {
                    stream::iter_ok(lip.iter()).and_then(|ip| {
                        tx.send(ip)
                    }).into()
                })
        }));
        // interleave v4 and v6

        rx
    }
    pub fn resolve(
        &self,
        host: &'static str,
    ) -> ResolveResult<(impl Future<Item = LookupIp, Error = ResolveError>, impl Future<Item = LookupIp, Error = ResolveError>)> {
        debug!("resolving: {}", host);
        let (conf, opts) = read_system_conf()?;
        // read_system_conf just returns the ::default opts
        let mut v4_opts = opts.clone();
        v4_opts.ip_strategy = LookupIpStrategy::Ipv4Only;
        let v4_fut = ResolverFuture::new(conf.clone(), v4_opts)
            .and_then(|r| r.lookup_ip(host));

        let mut v6_opts = opts.clone();
        v6_opts.ip_strategy = LookupIpStrategy::Ipv6Only;
        let v6_fut = ResolverFuture::new(conf.clone(), v6_opts)
            .and_then(|r| r.lookup_ip(host));

        Ok((v4_fut, v6_fut))
    }
    pub fn connect(
        &self,
        socks: impl Stream<Item = SocketAddr, Error = ResolveError>,

    ) -> Box<Future<Item = TcpStream, Error = Box<std::error::Error>>> {
        debug!("Connect");
        // let now = Instant::now();
        let (tx, rx) = futures::sync::mpsc::channel(1);
        stream::iter_ok(1..).zip(socks).for_each(|(i, s)| {
            let offset = ((i as u64) + 1) * self.conn_attempt_delay as u64;
            let when = Instant::now() + Duration::from_millis(offset);
            let txc = tx.clone();
            debug!("creating delay of {}", offset);
            Delay::new(when)
                .map_err(|e| panic!("delay errored; err={:?}", e))
                .and_then(move |_| {
                    debug!("[]happy eyeballs start connect: {}", s);
                    TcpStream::connect(&s).map_err(|e| panic!("delay errored; err={:?}", e))
                })
                .and_then(move |c| {
                    debug!("happy eyeballs connected: {:?}", c);
                    txc.send(c)
                        .and_then(|_| Ok(()))
                        .map_err(|e| panic!("delay errored; err={:?}", e))
                })
        });
        Box::new(
            rx.into_future()
                .map(|x| {
                    debug!("happy eyeballs recv: {:?}", x.0);
                    x.0.unwrap()
                })
                .map_err(|e| panic!("delay errored; err={:?}", e)),
        )
    }
}
