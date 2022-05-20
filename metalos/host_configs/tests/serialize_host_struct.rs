use anyhow::Result;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use clap::{ArgEnum, Parser};
use example_host_for_tests::example_host_for_tests;
use metalos_host_configs::host::HostConfig;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    example: bool,
    #[clap(arg_enum, default_value_t=Piece::Host)]
    piece: Piece,
}

#[derive(ArgEnum, Clone, Debug)]
enum Piece {
    Host,
    Provisioning,
    Boot,
    Runtime,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let h: HostConfig = match args.example {
        true => example_host_for_tests(),
        false => {
            let mut writer = BytesMut::new().writer();
            std::io::copy(&mut std::io::stdin(), &mut writer)?;
            let input: Bytes = writer.into_inner().into();
            fbthrift::simplejson_protocol::deserialize(&input)?
        }
    };

    let out_json = match args.piece {
        Piece::Host => fbthrift::simplejson_protocol::serialize(&h),
        Piece::Provisioning => fbthrift::simplejson_protocol::serialize(&h.provisioning_config),
        Piece::Boot => fbthrift::simplejson_protocol::serialize(&h.boot_config),
        Piece::Runtime => fbthrift::simplejson_protocol::serialize(&h.runtime_config),
    };
    std::io::copy(&mut out_json.reader(), &mut std::io::stdout())?;
    Ok(())
}
