use anyhow::Result;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use metalos_host_configs::host::HostConfig;

fn main() -> Result<()> {
    let mut writer = BytesMut::new().writer();
    std::io::copy(&mut std::io::stdin(), &mut writer)?;
    let input: Bytes = writer.into_inner().into();
    let h: HostConfig = fbthrift::simplejson_protocol::deserialize(&input)?;
    let json = fbthrift::simplejson_protocol::serialize(&h);
    std::io::copy(&mut json.reader(), &mut std::io::stdout())?;
    Ok(())
}
