def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        pw_hashes={
            "root": prov.root_pw_hash,
        }
    )
