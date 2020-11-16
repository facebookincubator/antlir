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
    Introduction: ['introduction', 'faqs'],
    Tutorials: ['tutorials/defining-an-image'],
    API: [
      {
        'Building Images': [
          'api/building-images/image-layer',
          {'Image Actions': ['api/building-images/image-actions/make-dirs']},
        ],
        'nspawn Runtime': ['api/building-images/nspawn-runtime/image-unittest'],
        'VM Runtime': ['api/building-images/vm-runtime/vm-unittest'],
      },
      'api/shape',
    ],
    'Concepts & Design': [
      {
        RPMs: [
          'concepts/rpms/using-rpms-in-images',
          'concepts/rpms/version-selection',
          'concepts/rpms/overview',
        ],
        'Pre-built Artifacts': [
          {
            Fbpkgs: [
              ...fbInternalOnly([
                'concepts/pre-built-artifacts/fb/fbpkgs/overview',
                'concepts/pre-built-artifacts/fb/fbpkgs/using-fbpkgs-in-images',
                'concepts/pre-built-artifacts/fb/fbpkgs/fbpkg-fetched-buck-macros',
              ]),
            ],
          },
        ],
      },
    ],
    Contributing: [
      ...fbInternalOnly(['contributing/fb/contributing-docs']),
      {
        'Coding Conventions': [
          'contributing/coding-conventions/bzl-and-targets',
          'contributing/coding-conventions/pyre',
          'contributing/coding-conventions/python',
        ],
        TODOs: ['contributing/todos/btrfs_diff', 'contributing/todos/compiler'],
      },
    ],
    Appendix: ['appendix/vision-containers-as-build-artifacts'],
  },
};
