---
sidebar_position: 3
---

# RPMs

## Snapshot

We take full snapshots of all available yum repositories and periodically update
the in-repo copy.

<FbInternalOnly>
See <a href="../fb/fast-snapshot">fast-snapshot</a> for how this works internally.
</FbInternalOnly>

### Buck targets

There are a number of buck2 rules that are used in the snapshot that enable
incremental updates of rpm repos

#### rpm

The `rpm` rule describes a single rpm. It holds the `nevra`, `xml` repodata
chunks and the actual `.rpm` blob.

#### repo

The `repo` rule includes zero or more `rpm` dependencies, and produces a
`repodata` directory that as a full `xml` representation of all the repository
metadata.

#### repo_set

The `repo_set` rule contains zero or more `repo` dependencies, and serves as a
convenient grouping to refer to all repos that are part of a distribution with a
single buck target label. This rule does not build anything on its own.

## Installation

The user api of `feature.rpms_install(rpms=["foobarbaz"])` requires a lot more
process than the simple api suggests.

### Resolution

The first step is dependency resolution. `foobarbaz` might depend on any number
of other rpms, that are either already installed or need to be downloaded along
with their dependencies.

Obviously, resolving package manager transactions is well outside the scope of
`antlir2`, but we do need to be able to interact with these transactions.

Luckily, `dnf` provides a python api that allows us to resolve and inspect a
transaction without actually installing it.

`antlir2` takes an ephemeral snapshot of the `parent_layer`, then uses the `dnf`
api within that container. `antlir2` provides a directory with all the repodatas
as produced by [the rule](#repo), which `dnf` can use to read all the metadata.
`antlir2` feeds the desired user actions (install, upgrade or remove) into `dnf`
and `dnf` returns a resolved transaction (or an error if the user request was
impossible).

`antlir2` then passes that resolution back to the `buck2` rules.

### Materialization

We never provide build containers with network access (especially upstream tools
like `dnf` that are not at all built for determinism), as that would make
guaranteeing [reproducibility](../../reproducibility) almost impossible.

But, we need to get these `.rpm`s materialized onto disk somehow so that `dnf`
can install them.

Luckily, `buck2` has
[dynamic dependencies](https://buck2.build/docs/rule_authors/dynamic_dependencies/)
which let an `image.layer` take a "dep" on every single rpm that's made
available to it, but only selectively materialize the ones that are actually
going to be installed in a transaction.

### Installation

Now that all the `repodata` and needed `.rpm`s have been locally materialized,
`antlir2` re-invokes the `dnf` api to actually do the already-resolved
transaction.

`dnf` does not handle its own history db very well, so `antlir2` is very careful
to make sure that the "install reason" for every single rpm is correct,
otherwise our safety guarantee of explicitly-installed rpms never being
implicitly removed later on cannot be upheld.

For example:

- `foobarbaz` depends on `foobar` which depends on `foo`
- child layer installs `foobar` for its own needs, and removes `foobarbaz`
- later `image.layer` tries to remove `foo`
  - this must fail

### DNF Repository Selection

By default `image.layer` gets access only to a set of main DNF repositories that inclues base
CentOS repos, Hyperscale repo and EPEL repo. Additional repositores can be added using `dnf_additional_repos`
API parameter of `image.layer` macro. For example if you want to install Cuda rpms you need to set that
parameter to `["nvidia"]`. You can use following Buck query to find which repo includes a given rpm:

```
$NAME=cuda
$ buck2 uquery --output-attribute='logical_id' 'kind(repo, rdeps(fbcode//bot_generated/antlir/rpm/fast_snapshot/repos/..., attrregexfilter(nevra_name, "^%s$", fbcode//bot_generated/antlir/rpm/fast_snapshot/rpms/...), 1))' $NAME | jq -r '.[] | .logical_id' | sort -u
Buck UI: https://www.internalfb.com/buck2/0981a319-9282-4347-a028-08af7ee8f54b
Network: Up: 0B  Down: 0B
Jobs completed: 15680. Time elapsed: 59.6s.
[2024-02-07T12:28:10.360-08:00] Network: Up: 0B  Down: 0B
nvidia
```
