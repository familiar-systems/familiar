export const siteConfig = {
  name: "familiar.systems",
  description: "AI-assisted campaign notebook for tabletop RPG game masters.",
  logo: {
    src: "/raven-icon.svg",
    srcDark: "/raven-icon.svg",
    alt: "familiar.systems",
    strategy: "invert" as "invert" | "switch" | "static",
  },
  ogImage: "",
  primaryColor: "#5a4a6a",
  search: {
    enabled: true,
  },
  announcement: {
    enabled: false,
    id: "launch_v1",
    link: "/changelog",
    localizeLink: true,
  },
  blog: {
    postsPerPage: 6,
  },
  legal: {
    entityName: "Grinshpon Consulting ENK",
    orgNumber: "936 927 742 MVA",
  },
  contact: {
    email: {
      support: "hello@familiar.systems",
      sales: "hello@familiar.systems",
    },
    phone: {
      main: "",
      label: "",
    },
    address: {
      city: "",
      full: "",
    },
  },
  analytics: {
    alwaysLoad: false,
    vendors: {
      googleAnalytics: {
        id: "",
        enabled: false,
      },
      rybbit: {
        id: "",
        src: "",
        enabled: false,
      },
      umami: {
        id: "",
        src: "",
        enabled: false,
      },
    },
  },
  dateOptions: {
    localeMapping: {
      en: "en-US",
    },
  },
};

export const NAV_LINKS = [
  {
    href: "/roadmap",
    label: "Roadmap",
  },
  {
    href: "/blog",
    label: "Blog",
  },
  {
    href: "/about",
    label: "About",
  },
];

export const ACTION_LINKS = {
  primary: { label: "Read the Vision", href: "/blog/2026-02-20-the-vision" },
  social: {
    github: "https://github.com/loreweaver-no",
  },
};

export const FOOTER_LINKS = {
  project: {
    title: "Project",
    links: [
      { href: "/roadmap", label: "Roadmap" },
      { href: "/blog", label: "Blog" },
      { href: "/about", label: "About" },
    ],
  },
  legal: {
    title: "Legal",
    links: [
      { href: "/privacy", label: "Privacy" },
      { href: "/terms", label: "Terms" },
      { href: "/license", label: "License" },
      { href: "/sub-processors", label: "Sub-processors" },
    ],
  },
};
