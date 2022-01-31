def generator(host: metalos.HostIdentity) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/etc/hostname", contents=host.hostname + "\n"),
        ]
    )
