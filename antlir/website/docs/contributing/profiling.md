---
id: profiling
title: Profiling the Compiler
---

## Why

Profiling the Antlir compiler is very important for being able to make targeted performance improvements, as well as being able to prove performance gains/regressions.

## Getting profiler data

The Antlir compiler respects an env var `ANTLIR_PROFILE`. When `ANTLIR_PROFILE` is set to a directory path, the compiler will write out a Python pstat file from `cProfile` for each image target.

```s
$ ANTLIR_PROFILE=/home/antliruser/prof buck build //my/image:target
$ ls /home/antliruser/prof
my_image:target.pstat
```

## Viewing profiler data

There are a variety of things you can do with pstat files, but the easiest is probably to load it in [snakeviz](https://jiffyclub.github.io/snakeviz/).

<FbInternalOnly>

```s
$ buck run //antlir/third-party/facebook:snakeviz -- -h $HOSTNAME -s /home/antliruser/my_image:target.pstat
```

</FbInternalOnly>
<OssOnly>

```s
$ snakeviz /home/antliruser/my_image:target.pstat
```

</OssOnly>

This will print out a URL that you can visit in your browser to get an explorable icicle graph. If you're unable to load the site in Chrome, trying a different browser (ex: Safari) may solve the problem.

## Caveats

This doesn't give a completely accurate representation of actual wall-clock time spent in the compiler. Normal operation of the compiler will run parallelizable `ImageItem`s in threads, but profiling mutlithreaded programs is not well supported by cProfile, so threading is disabled when `ANTLIR_PROFILE` is set. The resulting profile data thus does not adequately show which steps the compiler would normally take in parallel, but it does still show accurate timings for the individual actions.
