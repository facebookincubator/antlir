def generator(host: metalos.HostIdentity) -> metalos.GeneratorOutput.type:
    return metalos.GeneratorOutput(
        pw_hashes = {
            "root": host.root_pw_hash,
        }
    )
