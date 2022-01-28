def generator(host: metalos.HostIdentity) -> metalos.GeneratorOutput.type:
    return metalos.GeneratorOutput(
        files=[
            metalos.file(path="/etc/hostname", contents=host.hostname + "\n"),
        ]
    )
