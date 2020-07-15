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
  title: FBInternalWithOssFallback('FS_Image @FB', 'FS_Image'),
  tagline: 'A filesystem image builder',
  url: FBInternalWithOssFallback(
    'https://www.internalfb.com/intern/staticdocs/fs_image',
    'https://www.github.com/facebookincubator/fs_image',
  ),
  baseUrl: '/',
  favicon: 'img/favicon.ico',
  organizationName: 'facebookincubator', // Usually your GitHub org/user name.
  projectName: 'fs_image', // Usually your repo name.
  themeConfig: {
    navbar: {
      title: FBInternalWithOssFallback('FS_Image @FB', 'FS_Image'),
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
          href: 'https://github.com/facebookincubator/fs_image',
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
              to: 'https://github.com/facebookincubator/fs_image',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'Stack Overflow',
              href: 'https://stackoverflow.com/questions/tagged/fs_image',
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
          editUrl:
            'https://github.com/facebookincubator/fs_image/edit/master/website/',
        },
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      },
    ],
  ],
  plugins: [require.resolve('docusaurus-plugin-internaldocs-fb')],
};
