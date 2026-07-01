use std::fmt;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ureq::Agent;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, RustlsConnector, Transport,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn config() -> Config {
    Config::builder().timeout_global(Some(TIMEOUT)).build()
}

pub trait Stream: Read + Write + Send + Sync + 'static {}
impl<T: Read + Write + Send + Sync + 'static> Stream for T {}

pub trait Dialer: Send + Sync + 'static {
    fn dial(&self, host: &str, port: u16) -> io::Result<Box<dyn Stream>>;
}

pub fn local_agent() -> Agent {
    Agent::new_with_config(config())
}

pub fn dialer_agent(dialer: Arc<dyn Dialer>) -> Agent {
    let connector = DialConnector { dialer }.chain(RustlsConnector::default());
    Agent::with_parts(config(), connector, NullResolver)
}

pub fn get(agent: &Agent, url: &str) -> Result<Vec<u8>, ureq::Error> {
    agent.get(url).call()?.body_mut().read_to_vec()
}

pub fn get_large(agent: &Agent, url: &str) -> Result<Vec<u8>, ureq::Error> {
    agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(DOWNLOAD_LIMIT)
        .read_to_vec()
}

struct StreamTransport {
    stream: Box<dyn Stream>,
    buffers: LazyBuffers,
}

impl fmt::Debug for StreamTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StreamTransport")
    }
}

impl Transport for StreamTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, _timeout: NextTimeout) -> Result<(), ureq::Error> {
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        self.stream.flush()?;
        Ok(())
    }

    fn await_input(&mut self, _timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        true
    }
}

struct DialConnector {
    dialer: Arc<dyn Dialer>,
}

impl fmt::Debug for DialConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DialConnector")
    }
}

impl<In: Transport> Connector<In> for DialConnector {
    type Out = StreamTransport;

    fn connect(
        &self,
        details: &ConnectionDetails,
        _chained: Option<In>,
    ) -> Result<Option<StreamTransport>, ureq::Error> {
        let uri = details.uri;
        let host = uri.host().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "request URI has no host")
        })?;
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            });
        let stream = self.dialer.dial(host, port)?;
        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );
        Ok(Some(StreamTransport { stream, buffers }))
    }
}

#[derive(Debug)]
struct NullResolver;

impl Resolver for NullResolver {
    fn resolve(
        &self,
        uri: &Uri,
        _config: &Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let port = uri.port_u16().unwrap_or(0);
        let mut addrs = ResolvedSocketAddrs::from_fn(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        addrs.push(SocketAddr::from(([0, 0, 0, 0], port)));
        Ok(addrs)
    }
}
