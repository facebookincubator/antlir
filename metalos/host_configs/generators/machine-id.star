def generator(host: metalos.HostIdentity) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/etc/machine-id", contents=host.id + "\n"),
        ]
    )
