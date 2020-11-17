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
    Introduction: ['introduction', 'faq', ...fbInternalOnly(['fb/faq'])],
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
          ...fbInternalOnly(['concepts/rpms/fb/how-rpms-are-updated']),
          'concepts/rpms/how-rpms-are-updated',
          'concepts/rpms/using-rpms-in-images',
          'concepts/rpms/version-selection',
        ],
        'Pre-built Artifacts': [
          {
            Fbpkgs: [
              ...fbInternalOnly([
                'concepts/pre-built-artifacts/fb/fbpkgs/how-fbpkgs-are-updated',
                'concepts/pre-built-artifacts/fb/fbpkgs/using-fbpkgs-in-images',
                // 'concepts/pre-built-artifacts/fb/fbpkgs/images-as-fbpkgs',
              ]),
            ],
          },
          // 'concepts/pre-built-artifacts//fetched-artifacts',
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
