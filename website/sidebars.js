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
    TODOs: ['todo/overview', 'todo/btrfs_diff', 'todo/compiler'],
    Tutorials: [...FBInternalOnly(['tutorials/fb/contributing-docs'])],
    RPMs: [
      ...FBInternalOnly([
        'rpms/fb/overview',
        'rpms/fb/version-selection-in-buck-built-images',
      ]),
    ],
    ...FBInternalOnly({
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
