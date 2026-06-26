// Step 2: scriptorium variant of the system picker.
//
// Two parts:
//   1. Search-first combobox over the catalog's `systems` array.
//   2. An always-visible "Bring your own" card below the search. BYO is
//      a first-class affordance, not a fallback. Picking it surfaces an
//      optional custom-name input; the wizard resolves a single
//      `game_system` string at initialization time (catalog pick name | trimmed
//      BYO name | BYO_DEFAULT_NAME). No magic slugs cross the wire.
//
// The BYO card's UI copy lives here as hardcoded English, matching the
// rest of the wizard. The catalog response only carries BYO's default
// template bundle, since that is the only piece of BYO a catalog
// maintainer configures.
//
// Once a system or BYO is picked the bundle templates are pre-selected.
// The user can toggle individual templates on/off for catalog systems.
// (BYO presents no per-template toggle in v0; the BYO bundle ships as-is.)

import type { CatalogResponse, SystemEntry, TemplateRef } from "@familiar-systems/types-campaign";
import { Check, Plus, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { m } from "../../paraglide/messages.js";
import { fuzzyMatchSystems } from "./fuzzyMatch";

/**
 * BYO display label used when the user picks BYO and leaves the optional
 * custom-name input empty. Shipped verbatim as `game_system` on the
 * initialize payload.
 */
export const BYO_DEFAULT_NAME = "Custom";
/** Hex color used for BYO's card swatch and the review-step swatch. */
export const BYO_COLOR = "#5a4a6a";

/**
 * The user's choice on the system step. A sum type so the wizard never
 * holds "BYO + a catalog id" or other nonsense combinations.
 */
export type SystemPick =
  | { kind: "none" }
  | { kind: "catalog"; entry: SystemEntry }
  | { kind: "byo"; name: string };

interface StepSystemProps {
  catalog: CatalogResponse | null;
  pick: SystemPick;
  selectedTemplateSlugs: ReadonlySet<string>;
  onCatalogPick: (system: SystemEntry) => void;
  onByoPick: () => void;
  onByoNameChange: (next: string) => void;
  onTemplatesChange: (next: Set<string>) => void;
}

export function StepSystem({
  catalog,
  pick,
  selectedTemplateSlugs,
  onCatalogPick,
  onByoPick,
  onByoNameChange,
  onTemplatesChange,
}: StepSystemProps): React.ReactElement {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const systems = catalog?.systems ?? null;

  const matches = useMemo(() => {
    if (systems === null) return [];
    return fuzzyMatchSystems(systems, query);
  }, [systems, query]);

  const popular = useMemo(() => {
    if (systems === null) return [];
    return systems.filter((s) => s.popular);
  }, [systems]);

  const showPopular = query.trim() === "";
  const list = showPopular ? popular : matches;
  const selectedCatalog = pick.kind === "catalog" ? pick.entry : null;
  const byoSelected = pick.kind === "byo";
  const byoName = pick.kind === "byo" ? pick.name : "";

  return (
    <div className="space-y-6 enter-from-below">
      <header className="space-y-3">
        <p className="text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase">
          {m.stepSystemEyebrow()}
        </p>
        {/* Headline stays inline English: the gold-emphasized "system" is
            inline markup Paraglide's plain-string messages can't carry yet;
            localized with a rich-text helper (Phase 4). */}
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          Choose your <em className="text-gold italic">system</em>.
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          {m.stepSystemLede()}
        </p>
      </header>

      <div className="relative">
        <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <input
          ref={inputRef}
          data-testid="system-search-input"
          type="text"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
          }}
          placeholder={m.stepSystemSearchPlaceholder()}
          className="w-full rounded-xl border border-foreground/10 bg-background/60 py-3 pr-4 pl-10 text-base text-foreground placeholder:text-muted-foreground/60 focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none"
        />
      </div>

      {catalog === null ? (
        <p data-testid="catalog-loading" className="text-sm text-muted-foreground">
          {m.stepSystemCatalogLoading()}
        </p>
      ) : (
        <>
          {showPopular ? (
            <p className="text-xs tracking-[0.18em] text-muted-foreground/70 uppercase">
              {m.stepSystemMostPlayed()}
            </p>
          ) : null}

          <div data-testid="system-list" className="grid gap-3 md:grid-cols-2">
            {list.length === 0 ? (
              <p className="col-span-full text-sm text-muted-foreground">{m.stepSystemNoMatch()}</p>
            ) : null}
            {list.map((system) => (
              <SystemCard
                key={system.id}
                system={system}
                selected={selectedCatalog?.id === system.id}
                onClick={() => {
                  onCatalogPick(system);
                }}
              />
            ))}
          </div>

          {selectedCatalog !== null ? (
            <TemplatesEditor
              system={selectedCatalog}
              selectedSlugs={selectedTemplateSlugs}
              onTemplatesChange={onTemplatesChange}
            />
          ) : null}

          <BringYourOwn
            selected={byoSelected}
            customName={byoName}
            onPick={onByoPick}
            onCustomNameChange={onByoNameChange}
          />
        </>
      )}
    </div>
  );
}

interface SystemCardProps {
  system: SystemEntry;
  selected: boolean;
  onClick: () => void;
}

function SystemCard({ system, selected, onClick }: SystemCardProps): React.ReactElement {
  return (
    <button
      type="button"
      data-testid={`system-card-${system.id}`}
      data-selected={selected}
      onClick={onClick}
      className={[
        "group relative flex flex-col gap-2 rounded-2xl border p-4 text-left transition-all duration-200",
        selected
          ? "border-gold/60 bg-bronze-muted/30 shadow-md shadow-gold/10"
          : "border-foreground/10 bg-background/40 hover:-translate-y-0.5 hover:border-primary/30 hover:shadow-md hover:shadow-primary/10",
      ].join(" ")}
    >
      <span className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className="size-3 rounded-full ring-2 ring-background"
          style={{ background: system.color }}
        />
        <span className="font-display text-lg font-medium tracking-tight">{system.name}</span>
      </span>
      <span className="text-sm leading-snug text-muted-foreground">{system.tagline}</span>
    </button>
  );
}

interface TemplatesEditorProps {
  system: SystemEntry;
  selectedSlugs: ReadonlySet<string>;
  onTemplatesChange: (next: Set<string>) => void;
}

function TemplatesEditor({
  system,
  selectedSlugs,
  onTemplatesChange,
}: TemplatesEditorProps): React.ReactElement {
  return (
    <div
      data-testid="templates-editor"
      className="space-y-3 rounded-2xl border border-foreground/10 bg-bronze-muted/20 p-5"
    >
      <header className="space-y-1">
        <h3 className="font-display text-lg font-medium tracking-tight">
          {m.stepSystemTemplatesHeading()}
        </h3>
        <p className="text-sm text-muted-foreground">
          {m.stepSystemTemplatesLede({ systemName: system.name })}
        </p>
      </header>
      <div className="flex flex-wrap gap-2">
        {system.bundle.map((tpl: TemplateRef) => {
          const checked = selectedSlugs.has(tpl.slug);
          return (
            <button
              key={tpl.slug}
              type="button"
              data-testid={`template-toggle-${tpl.slug}`}
              data-selected={checked}
              onClick={() => {
                const next = new Set(selectedSlugs);
                if (checked) {
                  next.delete(tpl.slug);
                } else {
                  next.add(tpl.slug);
                }
                onTemplatesChange(next);
              }}
              className={[
                "inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-medium transition-colors",
                checked
                  ? "border-gold/60 bg-gold/10 text-gold"
                  : "border-foreground/10 bg-background/40 text-muted-foreground hover:border-primary/30 hover:text-foreground",
              ].join(" ")}
            >
              {checked ? (
                <Check aria-hidden="true" className="size-3" />
              ) : (
                <Plus aria-hidden="true" className="size-3" />
              )}
              <span>{tpl.name}</span>
            </button>
          );
        })}
      </div>
      <p className="text-xs text-muted-foreground/80">{m.stepSystemTemplatesFootnote()}</p>
    </div>
  );
}

interface BringYourOwnProps {
  selected: boolean;
  customName: string;
  onPick: () => void;
  onCustomNameChange: (next: string) => void;
}

// Always-visible BYO card. The copy is hardcoded here alongside the rest
// of the wizard's UI strings; the placeholder's empty-input fallback
// interpolates BYO_DEFAULT_NAME so the hint matches what the wire will
// actually send.
function BringYourOwn({
  selected,
  customName,
  onPick,
  onCustomNameChange,
}: BringYourOwnProps): React.ReactElement {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (selected) {
      inputRef.current?.focus();
    }
  }, [selected]);

  const placeholder = m.stepSystemByoNamePlaceholder({ byoDefault: BYO_DEFAULT_NAME });

  return (
    <div className="space-y-3" data-testid="bring-your-own">
      <p
        aria-hidden="true"
        className="flex items-center gap-3 text-xs tracking-[0.18em] text-muted-foreground/70 uppercase"
      >
        <span className="h-px flex-1 bg-foreground/10" />
        {m.stepSystemByoDivider()}
        <span className="h-px flex-1 bg-foreground/10" />
      </p>

      <button
        type="button"
        data-testid="byo-card"
        data-selected={selected}
        onClick={onPick}
        className={[
          "flex w-full items-start gap-3 rounded-2xl border p-4 text-left transition-all duration-200",
          selected
            ? "border-gold/60 bg-bronze-muted/30 shadow-md shadow-gold/10"
            : "border-foreground/10 bg-background/40 hover:-translate-y-0.5 hover:border-primary/30 hover:shadow-md hover:shadow-primary/10",
        ].join(" ")}
      >
        <span
          aria-hidden="true"
          className={[
            "mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border transition-colors",
            selected ? "border-gold bg-gold" : "border-foreground/20 bg-background",
          ].join(" ")}
        >
          {selected ? <span className="size-2 rounded-full bg-white" /> : null}
        </span>
        <span className="flex-1 space-y-2">
          <span className="flex flex-wrap items-baseline gap-2">
            <span className="font-display text-lg font-medium tracking-tight">
              {m.stepSystemByoTitle()}
            </span>
            <span className="inline-flex items-center rounded-full border border-foreground/10 bg-background/60 px-2 py-0.5 text-[10px] tracking-[0.18em] text-muted-foreground uppercase">
              {m.stepSystemByoBadge()}
            </span>
          </span>
          <span className="block text-sm leading-snug text-muted-foreground">
            {m.stepSystemByoDescription()}
          </span>
        </span>
      </button>

      {selected ? (
        <div className="space-y-2 rounded-2xl border border-foreground/10 bg-bronze-muted/20 p-4">
          <div className="flex items-baseline justify-between gap-4">
            <label htmlFor="byo-name" className="text-sm font-medium text-foreground">
              {m.stepSystemByoNameLabel()}
            </label>
            <span className="text-xs tracking-wider text-muted-foreground uppercase">
              {m.stepSystemByoNameOptional()}
            </span>
          </div>
          <input
            id="byo-name"
            ref={inputRef}
            data-testid="byo-name-input"
            type="text"
            value={customName}
            onChange={(e) => {
              onCustomNameChange(e.target.value);
            }}
            placeholder={placeholder}
            maxLength={60}
            className="w-full rounded-xl border border-foreground/10 bg-background/60 px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground/60 focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none"
          />
        </div>
      ) : null}
    </div>
  );
}
