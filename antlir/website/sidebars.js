/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

const {fbContent, fbInternalOnly} = require('docusaurus-plugin-internaldocs-fb/internal');

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
            ...fbInternalOnly([
              {
                Fbpkgs: [
                  'concepts/pre-built-artifacts/fb/fbpkgs/updating-fetched-fbpkgs',
                  'concepts/pre-built-artifacts/fb/fbpkgs/using-fbpkgs-in-images',
                ],
              },
            ]),
            'concepts/pre-built-artifacts/fetched-artifacts',
          ],
          Flavors: [
            'concepts/flavors/overview',
            'concepts/flavors/inheritance-in-parent-layers',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: 'API',
      collapsed: false,
      items: [
        'api/bzl/image.bzl',
        'api/bzl/image/feature/defs.bzl',
        'api/bzl/image/package/defs.bzl',
        'api/bzl/shape.bzl',
        'api/vm/bzl/defs.bzl',
        'api/bzl/test_rpms.bzl',
        'api/bzl/flavor_helpers.bzl',
        'genrule-layer',
        {
          'nspawn Runtime': ['runtime/nspawn-runtime/image-unittest'],
          'VM Runtime': ['runtime/vm-runtime/vm-unittest'],
        },
      ],
    },
    {
      type: 'category',
      label: 'Contributing',
      collapsed: true,
      items: [
        ...fbInternalOnly([
          'contributing/fb/contributing-docs',
          'contributing/fb/creating_a_flavor',
        ]),
        {
          'Coding Conventions': [
            'contributing/coding-conventions/bzl-and-targets',
            'contributing/coding-conventions/pyre',
            'contributing/coding-conventions/python',
            'contributing/coding-conventions/rust',
          ],
          TODOs: [
            'contributing/todos/btrfs_diff',
            'contributing/todos/compiler',
          ],
        },
        'contributing/profiling',
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
      {
        type: 'doc',
        id: 'fb/oss-testing',
      },
    ]),
  ],
};
