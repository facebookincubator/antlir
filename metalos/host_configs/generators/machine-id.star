def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/etc/machine-id", contents=prov.identity.id + "\n"),
        ]
    )
