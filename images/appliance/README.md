The images in this `appliance/` directory are used to build all of the images
in the `antlir` project, as well as user-defined image targets.
This way, `antlir` can easily make assumptions about the tooling available to
build an image, without relying on complicated host setup (or even an
rpm-based distribution)

stable-build-appliance
======================

`//images/appliance:stable-build-appliance` is the default appliance used to
build images. It comes from an artifact stored in S3, which provides big perf
wins (it takes ~5 minutes to rebuild this appliance image). The downside to
using a stable image is that it is slightly complicated to iterate on the
appliance image itself.

Building a new stable-build-appliance
-------------------------------------

The current process to build a new `stable-build-appliance` is via the
`host_build_appliance`. This requires `rpm` and friends to be present on the
build host, and likely other dependencies that are not yet explicitly
enumerated.

A new version of the appliance can easily be tested on image builds by passsing
`-c antlir.build-appliance-default=//images/appliance:rc-build-appliance`
to `buck build`.

Once satisfied, build a sendstream package and upload it to S3
```
$ buck build --show-output //images/appliance:rc-build-appliance.sendstream.zst
$ sendstream="buck-out/gen/images/appliance/rc-build-appliance.sendstream.zst/layer.sendstream.zst"
$ aws s3 cp "$sendstream" "s3://antlir/images/appliance/stable-build-appliance.sendstream.zst.$(sha256sum $sendstream)"
```
Finally, update the URL and `sha256` in `images/appliance/BUCK` to start
using the prebuilt stable image.
