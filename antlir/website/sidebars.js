/**
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

const {fbContent, fbInternalOnly} = require('internaldocs-fb-helpers');

module.exports = {
  docs: [
    {
      type: 'doc',
      id: 'overview',
    },
    ...fbInternalOnly([
      {
        type: 'doc',
        id: 'fb/getting_started',
      },
    ]),
    {
      type: 'doc',
      id: 'getting_started',
    },
    {
      type: 'doc',
      id: 'faq',
    },
    ...fbInternalOnly([
      {
        type: 'doc',
        id: 'fb/faq',
      },
    ]),
    {
      type: 'category',
      label: 'Tutorials',
      collapsed: false,
      items: ['tutorials/defining-an-image', 'tutorials/helper-buck-targets'],
    },
    {
      type: 'category',
      label: 'Concepts & Design',
      collapsed: false,
      items: [
        {
          RPMs: [
            'concepts/rpms/overview',
            ...fbInternalOnly(['concepts/rpms/fb/how-rpms-are-updated']),
            'concepts/rpms/how-rpms-are-updated',
            'concepts/rpms/using-rpms-in-images',
            'concepts/rpms/version-selection',
          ],
          'Pre-built Artifacts': [
            {
              Fbpkgs: [
                ...fbInternalOnly([
                  'concepts/pre-built-artifacts/fb/fbpkgs/updating-fetched-fbpkgs',
                  'concepts/pre-built-artifacts/fb/fbpkgs/using-fbpkgs-in-images',
                ]),
              ],
            },
            'concepts/pre-built-artifacts/fetched-artifacts',
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
        'genrule-layer',
        {
          'nspawn Runtime': ['runtime/nspawn-runtime/image-unittest'],
          'VM Runtime': ['runtime/vm-runtime/vm-unittest'],
        },
        'api/shape',
        ...fbInternalOnly(['api/genrule/facebook/chef_solo/chef_solo']),
        'api/flavor_helpers',
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
          TODOs: [
            'contributing/todos/btrfs_diff',
            'contributing/todos/compiler',
          ],
        },
      ],
    },
    ...fbContent({
      external: [
        // Towards the end since it's also linked from Getting Started
        {
          type: 'doc',
          id: 'installing',
        },
        {
          type: 'doc',
          id: 'oss-test-runner',
        },
      ],
    }),
    ...fbInternalOnly([
      {
        type: 'doc',
        id: 'fb/vision-containers-as-build-artifacts',
      },
    ]),
  ],
};
