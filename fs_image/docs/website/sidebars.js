/**
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

function FBInternalOnly(elements) {
  return process.env.FB_INTERNAL ? elements : [];
}

module.exports = {
  docs: {
    Introduction: ['introduction', 'why-build-containers-using-buck'],
    'Coding Conventions': [
      'coding-conventions/bzl-and-targets',
      'coding-conventions/pyre',
      'coding-conventions/python',
    ],
    Tutorials: [
      ...FBInternalOnly([
        'tutorials/fb/contributing-docs',
        'tutorials/fb/bumping-the-systemd-version-in-base-images',
      ]),
    ],
    RPMs: [
      ...FBInternalOnly([
        'rpms/fb/overview',
        'rpms/fb/debugging-stale-snapshots',
        'rpms/fb/version-selection-in-buck-built-images',
        'rpms/fb/mutable-rpm-investigation',
      ]),
    ],
    ...FBInternalOnly({
      Infrastructure: [
        'fb/infra/non-buck-target-determinators',
        'fb/infra/chronos-based-automation',
        'fb/infra/hiding-internal-edges-in-targets',
        'fb/infra/making-sure-older-builds-work',
      ],
      'Automatic Repo Updates': [
        'fb/automatic-repo-updates/overview',
        'fb/automatic-repo-updates/fbpkg-auto-preservation',
        'fb/automatic-repo-updates/handling-manual-rpm-snapshots',
      ],
      Fbpkg: [
        'fb/fbpkg/overview',
        {
          'Buck Macros': [
            'fb/fbpkg/fbpkg-fetched-buck-macros',
            {
              'fbpkg.builder': [
                'fb/fbpkg/fbpkg-builder-buck-macros/overview',
                'fb/fbpkg/fbpkg-builder-buck-macros/future-work',
              ],
            },
          ],
        },
      ],
    }),
    Appendix: ['appendix/vision-containers-as-build-artifacts'],
  },
};
