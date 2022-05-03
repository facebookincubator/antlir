use anyhow::Result;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use example_host_for_tests::example_host_for_tests;
use metalos_host_configs::host::HostConfig;

fn main() -> Result<()> {
    let mut args: Vec<_> = std::env::args().collect();
    args.remove(0);
    let h: HostConfig = match args.first().map(String::as_str) {
        Some("example") => example_host_for_tests(),
        Some(arg) => panic!("arg must be example or not set, not '{}'", arg),
        None => {
            let mut writer = BytesMut::new().writer();
            std::io::copy(&mut std::io::stdin(), &mut writer)?;
            let input: Bytes = writer.into_inner().into();
            fbthrift::simplejson_protocol::deserialize(&input)?
        }
    };
    let json = fbthrift::simplejson_protocol::serialize(&h);
    std::io::copy(&mut json.reader(), &mut std::io::stdout())?;
    Ok(())
}
