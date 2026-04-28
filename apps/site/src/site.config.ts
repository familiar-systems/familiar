export const siteConfig = {
  name: "familiar.systems",
  description:
    "AI-assisted campaign wiki for tabletop RPGs. Your world assembles from play. Your familiar takes notes, you play the game. Open Source.",
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
      support: "hello@loreweaver.no",
      sales: "hello@loreweaver.no",
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
    href: "/pricing",
    label: "Pricing",
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

// Required at build time. No fallback: a default would silently bake the
// dev URL into preview/prod artifacts. Local dev/build sets this in
// mise.toml; CI passes it as a Docker build-arg via build-site action.
const APP_URL = import.meta.env.PUBLIC_APP_URL;
if (!APP_URL) {
  throw new Error(
    "PUBLIC_APP_URL is required at site build time (target apex for the Sign In CTA).",
  );
}

export const ACTION_LINKS = {
  primary: { label: "Sign In", href: `${APP_URL}/login` },
  social: {
    github: "https://github.com/familiar-systems",
  },
};

export const FOOTER_LINKS = {
  project: {
    title: "Project",
    links: [
      { href: "/roadmap", label: "Roadmap" },
      { href: "/pricing", label: "Pricing" },
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
