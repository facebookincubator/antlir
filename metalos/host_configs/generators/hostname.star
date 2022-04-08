def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/etc/hostname", contents=prov.identity.hostname + "\n"),
        ]
    )
