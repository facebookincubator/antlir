/**
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

const {fbInternalOnly} = require('internaldocs-fb-helpers');

module.exports = {
  docs: {
    Introduction: ['introduction', 'why-build-containers-using-buck'],
    'Coding Conventions': [
      'coding-conventions/bzl-and-targets',
      'coding-conventions/pyre',
      'coding-conventions/python',
    ],
    TODOs: ['todo/overview', 'todo/btrfs_diff', 'todo/compiler'],
    Tutorials: [...fbInternalOnly(['tutorials/fb/contributing-docs'])],
    RPMs: [
      ...fbInternalOnly([
        'rpms/fb/overview',
        'rpms/fb/version-selection-in-buck-built-images',
      ]),
    ],
    ...fbInternalOnly({
      Fbpkg: [
        'fb/fbpkg/overview',
        {
          'Buck Macros': [
            'fb/fbpkg/fbpkg-fetched-buck-macros',
          ],
        },
      ],
    }),
    Appendix: ['appendix/vision-containers-as-build-artifacts'],
  },
};
