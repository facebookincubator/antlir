/**
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

function FBInternalWithOssFallback(elements, fallback) {
  return process.env.FB_INTERNAL ? elements : fallback;
}

module.exports = {
  title: FBInternalWithOssFallback('Antlir @FB', 'Antlir'),
  tagline: 'A filesystem image builder',
  url: FBInternalWithOssFallback(
    'https://www.internalfb.com/intern/staticdocs/antlir',
    'https://www.github.com/facebookincubator/antlir',
  ),
  baseUrl: '/',
  favicon: 'img/favicon.ico',
  organizationName: 'facebookincubator', // Usually your GitHub org/user name.
  projectName: 'antlir', // Usually your repo name.
  themeConfig: {
    navbar: {
      title: FBInternalWithOssFallback('Antlir @FB', 'Antlir'),
      logo: {
        alt: 'My Facebook Project Logo',
        src: 'img/logo.svg',
      },
      links: [
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
      '@docusaurus/preset-classic',
      {
        docs: {
          homePageId: 'introduction',
          sidebarPath: require.resolve('./sidebars.js'),
          editUrl: FBInternalWithOssFallback(
            'https://www.internalfb.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/docs/website',
            'https://github.com/facebookincubator/antlir/edit/master/website/',
          ),
        },
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      },
    ],
  ],
  plugins: [require.resolve('docusaurus-plugin-internaldocs-fb')],
};
