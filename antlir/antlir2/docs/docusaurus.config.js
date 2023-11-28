/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {fbContent} from 'docusaurus-plugin-internaldocs-fb/internal';
import {themes} from 'prism-react-renderer';

// With JSDoc @type annotations, IDEs can provide config autocompletion
/** @type {import('@docusaurus/types').DocusaurusConfig} */
(module.exports = {
  title: 'antlir2',
  tagline: 'Deterministic filesystem image builds with buck2',
  url: fbContent({
    internal: 'https://www.internalfb.com/',
    external: 'https://facebookincubator.github.io/',
  }),
  baseUrl: fbContent({
    internal: '/intern/staticdocs/antlir2/',
    external: '/antlir/antlir2/'
  }),
  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'throw',
  trailingSlash: true,
  favicon: 'img/favicon.ico',
  organizationName: 'facebookincubator',
  projectName: 'antlir',
  customFields: {
    fbRepoName: 'fbsource',
    ossRepoPath: 'fbcode/antlir/antlir2/docs',
  },

  presets: [
    [
      'docusaurus-plugin-internaldocs-fb/docusaurus-preset',
      /** @type {import('docusaurus-plugin-internaldocs-fb').PresetOptions} */
      ({
        docs: {
          sidebarPath: require.resolve('./sidebars.js'),
          editUrl: fbContent({
            internal: 'https://www.internalfb.com/code/fbsource/fbcode/antlir/antlir2/docs',
            external: 'https://github.com/facebookincubator/antlir/edit/master/antlir2/docs/',
          }),
        },
        experimentalXRepoSnippets: {
          baseDir: '.',
        },
        staticDocsProject: 'antlir2',
        trackingFile: 'xplat/staticdocs/WATCHED_FILES',
        blog: {
          showReadingTime: true,
          editUrl: fbContent({
            internal: 'https://www.internalfb.com/code/fbsource/fbcode/antlir/antlir2/docs',
            external: 'https://github.com/facebookincubator/antlir/edit/master/antlir2/docs/',
          }),
        },
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      navbar: {
        title: 'antlir2',
        logo: {
          alt: 'antlir2 Logo',
          src: 'img/logo.svg',
        },
        items: [
          {
            type: 'doc',
            docId: 'intro',
            position: 'left',
            label: 'Docs',
          },
          {to: '/blog', label: 'Blog', position: 'left'},
          {
            href: 'https://github.com/facebook/docusaurus',
            label: 'GitHub',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Community',
            items: fbContent({
              internal: [{
                label: 'Workplace Group',
                href: 'https://fb.workplace.com/groups/antlirusers',
              }],
              external: [{
                label: 'Stack Overflow',
                href: 'https://stackoverflow.com/questions/tagged/antlir',
              }],
            }),
          },
          {
            title: 'More',
            items: [
              {
                label: 'Blog',
                to: '/blog',
              },
              {
                label: 'GitHub',
                href: 'https://github.com/facebookincubator/antlir',
              },
            ],
          },
        ],
        copyright: `Copyright © ${new Date().getFullYear()} Meta Platforms, Inc. Built with Docusaurus.`,
      },
      prism: {
        theme: themes.github,
        darkTheme: themes.darcula,
      },
    }),
});
