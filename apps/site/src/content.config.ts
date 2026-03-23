import { defineCollection } from "astro:content";
import { z } from "astro/zod";
import { glob } from "astro/loaders";

const roadmapCollection = defineCollection({
  loader: glob({ pattern: "**/*.{md,mdx}", base: "./src/content/roadmap" }),
  schema: z.object({
    quest: z.number(),
    title: z.string(),
    description: z.string(),
    status: z.enum(["done", "in-progress", "planned"]).default("planned"),
    noindex: z.boolean().optional().default(false),
    nofollow: z.boolean().optional().default(false),
  }),
});

const blogCollection = defineCollection({
  loader: glob({ pattern: "**/*.{md,mdx}", base: "./src/content/blog" }),
  // Type-check frontmatter using a schema
  schema: ({ image }) =>
    z.object({
      title: z.string(),
      description: z.string(),
      author: z.string(),
      pubDate: z.coerce.date(),
      updatedDate: z.coerce.date().optional(),
      heroImage: image().optional(),
      tags: z.array(z.string()).optional(),
      youtubeId: z.string().optional(),
      audioUrl: z.string().optional(),
      isVideo: z.boolean().optional().default(false),
      noindex: z.boolean().optional().default(false),
      nofollow: z.boolean().optional().default(false),
    }),
});

export const collections = {
  blog: blogCollection,
  roadmap: roadmapCollection,
};
