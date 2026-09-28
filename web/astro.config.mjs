import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  output: 'static',
  integrations: [
    starlight({
      title: 'North documentation',
      description: 'Guides for people using and contributing to North.',
      sidebar: [
        {
          label: 'User guide',
          items: [{ autogenerate: { directory: 'user-guide' } }],
        },
        {
          label: 'Contributing',
          items: [{ autogenerate: { directory: 'contributing' } }],
        },
        { slug: 'changelog' },
      ],
    }),
  ],
});
