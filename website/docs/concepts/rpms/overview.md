---
id: overview
title: Overview
---

## RPMs come from snapshots

When you specify an RPM name in a Buck image specification, it does **not** come
directly from the production Yum repos. Instead, it is fetched from a repo
snapshot that is provided by your current checkout of FBCode.

### Where's the code?

-   Concurrent snapshots coordinate via
    [xdb.rpm_repos](https://our.intern.facebook.com/intern/db/shards_replicasets/xdb.rpm_repos/),
    run `db xdb.rpm_repos` for SQL queries
-   Blob data (RPM files, repodata files) are in the
    [Manifold bucket rpm_repos](https://our.intern.facebook.com/intern/manifold/bucket-view/?bucket_name=rpm_repos&show_keys=0)
-   Repo snapshotter & historical repo server:
    [fbcode/antlir/rpm/](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/antlir/rpm)
-   The manifold ID of the latest repo snapshot, plus scripts to generate and
    commit a new snapshot:
    [CentOS7](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/antlir/rpm/facebook/fb_centos7)
    and
    [CentOS8](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/antlir/rpm/facebook/fb_centos8).
    Inspect the snapshot via (e.g. for CentOS7):

    ``` {.sourceCode .sh}
    sqlite3 file:"$(readlink -f "$(
       buck build //antlir/rpm/facebook:fb_centos7 --show-full-output |
       cut -f 2 -d ' '
    )")"/snapshot/snapshot.sql3?mode=ro``
    ```

    For example, you can get stats on RPM errors via this query:

    ``` {.sourceCode .sh}
    SELECT "error", COUNT(1) FROM "rpm" WHERE "error" IS NOT NULL GROUP BY "error";
    ```

    As another example, let's see which versions of "netperf" rpm are available:

    ``` {.sourceCode .sh}
    SELECT * from "rpm" WHERE "name" IS "netperf";
    ```

## When are snapshots updated?

We automatically update snapshots on a roughly daily cadence, but this can
sometimes be delayed due to infrastructure issues. See
[Automatic Repo Updates](https://www.internalfb.com/intern/wiki/Tupperware/Engineering/Images/Internal_Automation/Automatic_Repo_Updates/).

### I have a SEV and need RPM `xyz-1.2.3` in my image RIGHT NOW

Don't despair, just follow these easy steps to clowntown:

-   Put the relevant RPM in `tupperware/image/your_team`, next to your image's
    `TARGETS` file.
-   Add `"image.source("xyz-1.2.3.rpm")` (this implicitly creates an
    `export_file` target) in your layer's RPMs list.

**Note:** If you need to **commit** a large RPM to fbcode instead of doing a
one-off sandbox hotfix, you should:

-   Put your RPM into an :fbFbpkg \<Fbpkg\> and tag it, - add the fbpkg to the
    FBCode fbpkg DB via
    `buck run antlir/fbpkg/facebook/db:update-db -- --db antlir/fbpkg/facebook/db/main_db/ --create YOUR_PKG YOUR_TAG '{}'`
-   use
    `image.source(fbpkg.fetched_layer('YOUR_PKG', 'YOUR_TAG'), path='your.rpm')`
    in your list of `rpms`, and you will install that specific RPM from your
    Fbpkg.

**Note2:** Having the versions in the TARGETS file and in the fbpkg makes it
significantly harder to update the versions. Specifically, you have to:

-   update & commit change to Configerator
-   update the TARGETS file
-   preserve a new fbpkg with the new versions
-   manually run update-db
-   commit all of the above as one diff
-   tag the fbpkg

In contrast, following the example of systemd in
`tupperware/image/base/os/TARGETS`, you have a 1.5-step approach:

-   preserve and tag the new fbpkg
-   wait for automation to commit it

### (For `twimage` team members only) Manual snapshot update

Snapshots are performed automatically, and should ideally be the only things
producing RPM repo updates. However, if for some reason you need to manually
snapshot, follow the steps below.

The strategy is to use the same script that's run by our automation,
[seen here](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/antlir/rpm/facebook/snapshot_and_upload.py).

A snapshot run on your devserver is likely to complete much faster compared to
those running in Sandcastle, and, as long as a previous snapshot has been
completed within the past few days, should take on the order of an hour.

To kick off a snapshot, simply run
`buck run //antlir/rpm/facebook/chronos:snapshot-and-upload`.

This script will publish a commit including everything neeeded to
publish a repo update. With this diff:

-   Make sure that contbuild is green. If not, investigate, and get help
    from the owning team.
-   Ship the diff

### Manually updating the build appliance

Image builds do not use RPM snapshots directly, rather, they need the snapshot
to be included into the "build appliance" image, which collects tools and data
required to build images.

The above `snapshot_and_upload` script should manually create and upload a new
build appliance. If for some reason you need to do this manually, you can simply
run:

``` {.sourceCode .sh}
buck run @mode/opt //tupperware/image/build_appliance:tupperware.image.sendstream.build_appliance
```

Next, take the ephemeral UUID that's printed and run:

``` {.sourceCode .sh}
buck run //antlir/fbpkg/facebook/db:update-db -- \
    --db "$(hg root)/fbcode/do-not-build/antlir/fbpkg/facebook/db/repo_tag_db/" \
    --no-update-existing \
    --replace tupperware.image.sendstream.build_appliance repo_stable \
    '{"uuid": "UUID", "allow_ephemeral": true, "auto_preserve": true}'
```

## FAQ

### My RPM exists in the repos, but fails to install from my `TARGETS` file

Troubleshooting steps (using CentOS7 as an example, though this also
applies to CentOS8):

-   Did you specify your RPM name correctly? Remember that `foo-project` is OK,
    but `foo-project-1.12` is not supported. We will eventually support version
    locking, but it will never use the RPM string syntax. For now, see "I have a
    SEV" above.
-   Does the repo snapshot in `antlir/rpm/facebook/fb_centos7/` include your
    RPM? Replace `gimp` with your RPM and run:

    ``` {.sourceCode .sh}
    echo 'SELECT "repo", "path" FROM "rpm" WHERE "name" = "gimp";' |
    sqlite3 file:"$(readlink -f "$(
    buck build //antlir/rpm/facebook:fb_centos7 --show-full-output |
       cut -f 2 -d ' '
    )")"/snapshot.sql3?mode=roo
    ```

If both steps above check out, the most likely cause is `mutable_rpm` errors.
For certain RPMs, human error has resulted in our repos having multiple copies
RPM with **different content** but the same name. See
[this post](https://fb.prod.workplace.com/groups/prodos.users/permalink/2612322622149670/)
for more info.

Since this is inherently broken, we disallow installing such RPMs from Buck.

If you see encounter this, you will want to:

-   Ping the OS team to remove the bad copy of the RPM, mentioning
    [this thread](https://fb.prod.workplace.com/groups/prodos.users/permalink/2612322622149670/)
    for context.
-   The OS team deletes the bad RPM that affects you (or ideally all!).
-   Ping the `twimage` team, who will add the *deleted* RPM to
    `facebook/deleted_mutable_rpms.py` and re-run the RPM snapshot for you.
-   Once you rebase, the "mutable_rpm" error should disappear and your RPM
    should install.

Note that as of November 2019, there is a firm intention to eliminate and
prevent `mutable_rpm` errors. Details in the comments on
[this post](https://fb.prod.workplace.com/groups/prodos.users/permalink/3009954169053178/).
