/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

const {fbContent} = require('docusaurus-plugin-internaldocs-fb/internal');
const path = require('path');

module.exports = {
  title: fbContent({
    internal: 'Antlir @FB',
    external: 'Antlir',
  }),
  tagline: 'A filesystem image builder',
  url: fbContent({
    internal: 'https://www.internalfb.com/intern/staticdocs/antlir',
    external: 'https://facebookincubator.github.io/antlir',
  }),
  baseUrl: '/antlir/',
  favicon: 'img/favicon.ico',
  organizationName: 'facebookincubator', // Usually your GitHub org/user name.
  projectName: 'antlir', // Usually your repo name.
  themeConfig: {
    navbar: {
      title: fbContent({
        internal: 'Antlir @FB',
        external: 'Antlir',
      }),
      logo: {
        alt: 'My Facebook Project Logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          to: 'docs/',
          activeBasePath: 'docs',
          label: 'Docs',
          position: 'left',
        },
        // Please keep GitHub link to the right for consistency.
        {
          href: 'https://github.com/facebookincubator/antlir',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Learn',
          items: [
            {
              label: 'Documentation',
              to: 'docs/',
            },
            {
              label: 'GitHub',
              to: 'https://github.com/facebookincubator/antlir',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'Stack Overflow',
              href: 'https://stackoverflow.com/questions/tagged/antlir',
            },
            /* TODO
            {
              label: 'Twitter',
              href: 'https://twitter.com/docusaurus',
            },
            {
              label: 'Discord',
              href: 'https://discordapp.com/invite/docusaurus',
            }, */
          ],
        },
        {
          title: 'Legal',
          // Please do not remove the privacy and terms, it's a legal requirement.
          items: [
            {
              label: 'Privacy',
              href: 'https://opensource.facebook.com/legal/privacy/',
              target: '_blank',
              rel: 'noreferrer noopener',
            },
            {
              label: 'Terms',
              href: 'https://opensource.facebook.com/legal/terms/',
              target: '_blank',
              rel: 'noreferrer noopener',
            },
          ],
        },
      ],
      logo: {
        alt: 'Facebook Open Source Logo',
        src: 'img/oss_logo.png',
        href: 'https://opensource.facebook.com',
      },
      // Please do not remove the credits, help to publicize Docusaurus :)
      copyright: `Copyright \u00A9 ${new Date().getFullYear()} Facebook, Inc. Built with Docusaurus.`,
    },
  },
  presets: [
    [
      require.resolve('docusaurus-plugin-internaldocs-fb/docusaurus-preset'),
      {
        docs: {
          sidebarPath: require.resolve('./sidebars.js'),
          editUrl: fbContent({
            internal:
              'https://www.internalfb.com/intern/diffusion/FBS/browse/master/fbcode/antlir/website',
            external:
              'https://github.com/facebookincubator/antlir/edit/master/website/',
          }),
          remarkPlugins: [require('mdx-mermaid')],
        },
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      },
    ],
  ],
  plugins: [path.resolve(__dirname, 'gen')],
};
