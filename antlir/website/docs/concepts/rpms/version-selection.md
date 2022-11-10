---
id: version-selection
title: Version Selection
---

## Constraining allowable package versions

Image builds [install RPMs from repo snapshots](overview.md#rpms-come-from-snapshots). These snapshots sometimes contain broken or bleeding-edge versions of packages.

We maintain automation, which periodically commits lists of allowable versions to fbcode for each _package group_, by evaluating a _version policy_ for the group. To address the fact that different applications may call for different package version mixes, fbcode may contain multiple named _version sets_, each picking different version policies. The italicized concepts are thoroughly explained below.

Versions for packages not included in any group will be whatever `yum` / `dnf` select, which is usually "latest" or "whatever is required by the package that depends on it".

**IMPORTANT:** Allowable versions for a package **ONLY** apply if your `image.layer` explicitly installs that package. So if your layer installs X, which depends on Y, and both have a version policy, the version policy for Y will only apply if your layer also explicitly installs Y. Otherwise, your version of X will be per policy, and Y will be whatever `yum` / `dnf` decide (i.e. latest acceptable per X's dependency spec). There's a [later section](#why-do-we-only-control-versions-for-explicitly-named-packages) explaining this rule.

## Concepts

### Package group, Universe, etc.

Start by reading [the overview](/docs/concepts/rpms/overview).

Package groups can be defined: manually, or via slowroll:

- `"packages": ["a", "b", "c"]`
- `"packages": {"source": "slowroll", "name": "systemd-packages"}` — this only works for multi-package slowrolls.

### Version policy

For a package group, a policy examines the versions available in the RPM snapshot, and looks at some other data source to pick the versions (possibly plural!) to allow installing into images. Available policies:

- A manual version spec: :

      {
         "policy": "manual",
         "versions": {"x86_64": [
            "VERSION1-RELEASE1",
            {"epoch": 3, "version": "VER2", "release": "REL2"},
            ...
         ]}
      }

  For each architecture, allows only the specified RPM epoch-version-releases to be installed for your package group. If you use the "version-release" shorthand, we try to will resolve the epoch from the RPM snapshot, and error if more than one package matches. You can specify additional architectures, e.g. `i686` is how we handle 32-bit binary support on CentOS7.

  The architecture can be the wildcard (`"*"`). Then RPMs of any architecture will pass version-selection criterion if epoch-version-release (or version-release shorthand if any) matches.

It is an error if none of the versions suggested by a policy are available in the snapshot.

When a policy encounters an error for a package, the package will "fail open", allowing the installation of any version. This behavior is deliberate — there is no value in preserving a stale version-lock, so we might as well have the diff check whether trunk now works without locks. The automatic diff description will suggest updating or deleting the corresponding package group policy to allow installing any version from the repo snapshot.

### Version set

We may want to build several flavors of images, with more or less well-tested package mixes. Every image is built using allowable versions from a specific version set name. The currently available names are:

- `tw_jobs`: Versions stable enough to safely include in a normal release process for a Tupperware job — e.g. a Conveyor / SF canary, followed by a gradual failure-reverting job update to use the new packages. Although each job owner is responsible for testing your job, avoid shipping something so unvetted that it will break push for many teams at once. This version set is not directly related to Slowroll `"stable"` — see [this section](https://our.intern.facebook.com/intern/wiki/Production_Engineering/OS/Slowroll/SlowrolledRPMsAndTW/#can-we-just-use-slowroll) for why using Slowroll versions outside of Chef makes no sense.

If a package group does not set a policy for a particular version set, any version in the RPM snapshot may be installed.

## How do I define a policy for my packages?

Decide whether your policy is specific for a flavor or it's good for all flavors. In the former case your `package_groups` directory will be `fbcode/antlir/rpm/allowed_versions/facebook/package_groups/<flavor>/`, in the latter case: `fbcode/antlir/rpm/allowed_versions/facebook/package_groups/`

Add `YOUR_GROUPNAME.json` into your `package_groups` directory with the following information:

    $ jq -C . systemd.json
    {
    "packages": {
       "source": "slowroll",
       "name": "systemd-packages"
    },
    "version_set_to_policy": {
       "tw_jobs": "slowroll"
    },
    "oncall": "systemd_releng"
    }

The `"packages"` field is documented above — for single-package slowrolls, you should just specify `"packages": ["rpm-name"]`.

Available policies & version set names are also documented above.

Suggested auto-formatting is `name=YOUR_FILE.json cfg="$(cat "$name")" ; echo "$cfg" | jq . > "$name"`. Whether for formatting, or for data model changes, we may modify your policy files without warning, while aiming to preserve their intent.

Update version files for each flavor affected:

```
for f in centos7 centos8 centos8-untested centos9 centos9-untested; do
  buck2 run antlir/rpm/allowed_versions:update-allowed-versions -- \
    --no-update-data-snapshot \
    --flavor $f \
    --version-sets-dir bot_generated/antlir/version_sets/$f \
    --data-snapshot-dir bot_generated/antlir/rpm/allowed_versions/snapshot/$f \
    --package-groups-dir antlir/rpm/allowed_versions/facebook/package_groups \
    --package-groups-dir antlir/rpm/allowed_versions/facebook/package_groups/$f \
    --rpm-repo-snapshot $(
      buck2 build antlir/rpm/facebook:$f --show-full-output |
      cut -d ' ' -f2)
done
```

At this point you can manually inspect updated version files (they are in `bot_generated/antlir/version_sets/<flavor>/tw_jobs/rpm/<oncall>/`) to see that they have proper ENVRAs and rebuild image layers locally for testing purposes.

Put up a diff with your policy and version files, then **wait for tests to be green**.

Once your policy and version files are committed, automation will begin regenerating a preferred version for your package group.

## What happens when a version updates?

Your oncall will see a diff that updates a file in fbcode to contain the new allowable versions for you package group. Automation will land this diff is all builds & tests are green. Otherwise, your oncall is responsible for ensuring that the builds & tests **become** green. On-diff failures typically indicate that your version bump broke somebody. Talk to that team, they'll be eager to help you fix them.

## How often do versions update?

We refresh versions more often than we generate RPM repo snapshots. The exact frequencies are subject to change, but snapshots will be ~1/week, and version refreshes will happen every couple of hours (limited by fbcode diff turnaround time).

However in some cases, a policy would only be able to pick versions that are not yet in the snapshot.

In these cases, the version bump will be delayed until the next RPM snapshot.

The practical rationale for the differing update frequencies is that testing a version bump is cheap, because Buck knows the full dependency graph for these. Testing a new snapshot involves rebuilding & retesting the world.

## How do allowable versions affect Buck dependencies?

Every `image.layer` that installs any RPMs depends on an RPM repo snapshot (normally, this is specified via the `build_appliance` field, formerly we used `yum_from_repo_snapshot`).

So, updating the snapshot rebuilds the world, which is expensive in terms of Sandcastle capacity.

In contrast, each package's "allowable versions" list is a separate target, and your `image.layer` only depends on the targets for RPMs it explicitly installs.

So, changing the version of a package (while leaving the snapshot fixed) will only rebuild those images that explicitly install the package.

## Why do we only control versions for explicitly named packages?

It seems pretty counterintuitive that for packages that are pulled in as dependencies, we go with "whatever `yum` / `dnf` wants" **even when** we know the subset of "better tested" versions for those packages.

We have a handful of reasons for having implicit versions be less locked down:

- **[most important]** We need package version bumps to be efficient in terms of Sandcastle capacity. This means only rebuilding images that are actually affected by the version bump. Buck currently does not know the dependency structure of RPMs, which means that we can only have efficient re-builds if version locks only affect those packages explicitly listed in `TARGETS` files. If version locks also affected implicitly installed packages, then any version bump would need to rebuild the world, which means that we would only be able to update versions at the time that we do repo snapshots, which is probably not what most teams want. We have a work item to try to expose the RPM dependency structure to Buck, which would enable more frequent snapshots, but I do not want to block the version selection work on that substantial effort. See "Map RPM dependencies onto Buck dependencies for efficient rebuilds on RPM updates" in https://fb.quip.com/YR5sAUGA74lc.
- It is completely plausible to lock down implicitly installed packages at a later date, if it proves to be a problem.
- If the latest "Y" is pulled in as a dependency of X, and the author of X is not satisfied with the version of "Y", then the dependency spec for X is wrong. We should be fixing these types of issues at the package level.
- Reducing the number of locked versions will generally reduce the operational headaches with resolving dependency conflicts.
- **[technicality]** This mirrors Chef's behavior, in the sense that if package `X` depends on `Y`, and both have slowrolls in recipes `rX` and `rY`, and somehow only `rX` is executed on your Chef run (i.e. `rX` does not explicitly depend on `rY`), then you will end up with `X` slowrolled, but `Y` unmanaged.

## Future: some version-locks are part of the snapshot, apply always

There is a narrow use-case, where the "latest" version in a repo snapshot is bad, and the breakage on hosts was mitigated using Slowroll or Chef. Specifically, this handles the "accidental VIM upgrade" scenario, where `vim` went to 8.0 before FB was ready for it.

For these, we'll want a separate version-lock file that is part of the snapshot, and that applies even to implicitly installed packages.

This is not hard to build, but we are not prioritizing it yet.
