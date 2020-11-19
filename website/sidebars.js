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
  docs: [
    {
      type: 'doc',
      id: 'introduction',
    },
    ...fbInternalOnly([{
      type: 'doc',
      id: 'fb/getting_started',
    }]),
    {
      type: 'doc',
      id: 'getting_started',
    },
    {
      type: 'doc',
      id: 'faq',
    },
    ...fbInternalOnly([{
      type: 'doc',
      id: 'fb/faq',
    }]),
    {
      type: 'category',
      label: 'Tutorials',
      collapsed: false,
      items: ['tutorials/defining-an-image'],
    },
    {
      type: 'category',
      label: 'Concepts & Design',
      collapsed: false,
      items: [
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
    },
    {
      type: 'category',
      label: 'API',
      collapsed: false,
      items: [
        'api/image',
        {
          'nspawn Runtime': ['runtime/nspawn-runtime/image-unittest'],
          'VM Runtime': ['runtime/vm-runtime/vm-unittest'],
        },
        'api/shape',
      ],
    },
    {
      type: 'category',
      label: 'Contributing',
      collapsed: true,
      items: [
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
    },
    // Towards the end since it's linked from Getting Started, and only
    // relevant to OSS users.
    {
      type: 'doc',
      id: 'installing',
    },
    {
      type: 'category',
      label: 'Appendix',
      collapsed: true,
      items: ['appendix/vision-containers-as-build-artifacts'],
    },
  ],
};
