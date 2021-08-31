def generator(host: metalos.Host) -> metalos.GeneratorOutput.type:
    return metalos.GeneratorOutput(
        files=[
            metalos.file(path="/etc/machine-id", contents=host.id + "\n"),
        ]
    )
