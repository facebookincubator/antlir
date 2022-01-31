def generator(host: metalos.HostIdentity) -> metalos.Output.type:
    return metalos.Output(
        pw_hashes = {
            "root": host.root_pw_hash,
        }
    )
