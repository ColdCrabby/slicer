import type { DetectionOption, DetectionQuestion } from '../../services/printer-connection';

/**
 * Wording for the setup questions detection raises, and the profile-level
 * effects the engine cannot express.
 *
 * The engine reports what a printer's config does and does not settle; it has
 * no opinion on how to phrase that. So the machine-specific half — which
 * options exist, which one the evidence points at, which config section it was
 * read from — arrives on the {@link DetectionQuestion}, and the plain-language
 * half lives here, keyed by the same ids.
 *
 * **Every option gets a description.** These questions are asked once, to
 * someone who has just plugged in a printer, and the option labels are terms
 * out of a config file. A label alone leaves "rscs" sitting next to "Not for
 * cooling prints" like a riddle; the sentence under it is the answer. The
 * wizard renders them as option cards for exactly that reason.
 *
 * Options whose effect is a slicing parameter carry it in `option.params` and
 * need no entry beyond wording. The ones listed in {@link PROFILE_EFFECTS}
 * change a *profile* field instead — a vendor, a model, a preferred plate
 * orientation — which is not something a params bag can express, so the wizard
 * applies those by id.
 */

/** Wording for one option. */
export interface OptionCopy {
  readonly label: string;
  readonly description?: string;
}

/** Wording for one question and its known options. */
export interface QuestionCopy {
  /** Step name in the wizard's progress caption. Two words or so. */
  readonly step: string | ((subject: string) => string);
  /**
   * The question itself, asked plainly — a function when the engine names what
   * it is about, so the headline can say "What is rscs for?" instead of "What
   * is this fan for?", which names nothing the user can go and look at.
   */
  readonly headline: string | ((subject: string) => string);
  /** Optional sentence under the headline. */
  readonly detail?: string;
  /** Labels and descriptions for options the engine cannot name. */
  readonly options: Readonly<Record<string, OptionCopy>>;
  /**
   * Wording for options the engine *did* name — a saved mesh profile. Receives
   * the engine's label and returns the card's own.
   */
  readonly nameOption?: (name: string) => OptionCopy;
  /** Link out for a term the question cannot avoid using. */
  readonly learnMore?: { readonly label: string; readonly url: string };
}

const KLIPPAIN_README_URL = 'https://github.com/Frix-x/klippain/blob/main/README.md';

const QUESTION_COPY: Readonly<Record<string, QuestionCopy>> = {
  machine_identity: {
    step: 'Which machine',
    headline: 'Is this the printer you have?',
    detail: 'Sets the make and model. It changes nothing about how your prints slice.',
    options: {
      confirm: {
        label: 'Yes, that is it',
        description: 'Saves the make and model so the profile is easy to spot.',
      },
      other: {
        label: 'No, something else',
        description: 'Keeps every setting we read; you name the machine yourself.',
      },
    },
  },
  macro_convention: {
    step: 'Start and end macros',
    headline: 'Which macro should we call to start a print?',
    detail:
      'We write one line at the top of every file and one at the bottom. Calling a macro your printer does not have stops the print on line one.',
    options: {
      standard: {
        label: 'PRINT_START and PRINT_END',
        description:
          'Mainline Klipper, and what most configs use. We pass bed and nozzle temperature.',
      },
      klippain: {
        label: 'START_PRINT and END_PRINT',
        description:
          'Klippain. We also pass chamber temperature and material name, which it expects.',
      },
      keep: {
        label: 'Neither — I write my own',
        description: 'We write nothing. The safe pick if you are unsure.',
      },
    },
    learnMore: { label: 'What is Klippain?', url: KLIPPAIN_README_URL },
  },
  bed_mesh: {
    step: 'Bed levelling',
    headline: 'Should we level the bed before each print?',
    detail: 'Your printer can probe. We cannot see whether your start macro already does.',
    options: {
      leave: {
        label: 'My start macro handles it',
        description: 'We send nothing. Almost every start macro probes already.',
      },
      calibrate: {
        label: 'Probe before every print',
        description:
          'Measures only the area the print covers. Adds a minute or two, and doubles up if your macro probes too.',
      },
    },
    nameOption: (name) => ({
      label: `Load the saved mesh “${name}”`,
      description: 'Instant, but only as fresh as the day you saved it.',
    }),
  },
  aux_fan: {
    step: (subject) => `The ${subject} fan`,
    headline: (subject) => `What is the “${subject}” fan for?`,
    detail:
      'Klipper names the part-cooling and hotend fans itself. Anything else could be a second part cooler, a filter, or a bay vent.',
    options: {
      unused: {
        label: 'Not for cooling prints',
        description: 'We never touch it. The safe pick if you are unsure.',
      },
      cooling: {
        label: 'It blows on the part',
        description: 'We ramp it with layer time, alongside the main part fan.',
      },
    },
  },
  preferred_orientation: {
    step: 'Part placement',
    headline: 'Should we place parts turned 45° on the plate?',
    detail: 'A preference, not something we read off the machine. It affects every plate.',
    options: {
      keep: {
        label: 'Place parts as they come',
        description: 'What almost everyone does.',
      },
      diagonal: {
        label: 'Turn every part 45°',
        description:
          'A CoreXY moves fastest on its diagonals. Costs usable plate area on a square bed.',
      },
    },
  },
};

/** Profile-field changes an answer implies, which a params bag cannot express. */
const PROFILE_EFFECTS: Readonly<
  Record<string, Readonly<Record<string, { preferred_orientation_deg?: number }>>>
> = {
  preferred_orientation: {
    keep: { preferred_orientation_deg: 0 },
    diagonal: { preferred_orientation_deg: 45 },
  },
};

/**
 * The copy key for a question id.
 *
 * A question about a named object carries the name in its id (`aux_fan:rscs`),
 * because a machine with two `[fan_generic]` sections asks twice and the two
 * answers must not collide. The wording is shared, so it is keyed on the part
 * before the colon.
 */
function copyKey(id: string): string {
  const colon = id.indexOf(':');
  return colon === -1 ? id : id.slice(0, colon);
}

/** Wording for a question id, or `null` for one this build doesn't know. */
export function questionCopy(id: string): QuestionCopy | null {
  return QUESTION_COPY[copyKey(id)] ?? null;
}

/** Resolve a headline or step name that may depend on the question's subject. */
function resolve(
  value: string | ((subject: string) => string),
  subject: string | null | undefined,
): string {
  return typeof value === 'string' ? value : value(subject ?? 'this one');
}

/** The question as a sentence, with whatever the engine named filled in. */
export function questionHeadline(question: DetectionQuestion, copy: QuestionCopy): string {
  return resolve(copy.headline, question.subject);
}

/** The step's name for the progress caption. */
export function questionStep(question: DetectionQuestion, copy: QuestionCopy): string {
  return resolve(copy.step, question.subject);
}

/**
 * Label for an option: this build's copy, or the engine's own phrasing for one
 * it named (a saved mesh profile), otherwise the bare id.
 */
export function optionLabel(question: DetectionQuestion, option: DetectionOption): string {
  const copy = questionCopy(question.id);
  if (option.label) {
    return copy?.nameOption?.(option.label).label ?? option.label;
  }
  return copy?.options[option.id]?.label ?? option.id;
}

/** The sentence under an option's label. */
export function optionDescription(
  question: DetectionQuestion,
  option: DetectionOption,
): string | undefined {
  const copy = questionCopy(question.id);
  if (option.label) {
    return copy?.nameOption?.(option.label).description ?? option.detail ?? undefined;
  }
  return copy?.options[option.id]?.description ?? option.detail ?? undefined;
}

/** Profile-field patch an answer implies, if any. */
export function optionProfilePatch(
  questionId: string,
  optionId: string,
): { preferred_orientation_deg?: number } | null {
  return PROFILE_EFFECTS[copyKey(questionId)]?.[optionId] ?? null;
}

/**
 * G-code template id an answer selects, or `null` when the answer is "leave my
 * macros alone" or the question isn't about macros.
 *
 * Template *copy* is engine-owned (`src/profiles/gcode_templates.rs`, surfaced
 * through `gcode-templates.ts`); the engine only ever reports which macros a
 * printer defines, and this maps that answer to a preset id.
 */
export function optionTemplateId(questionId: string, optionId: string): string | null {
  if (copyKey(questionId) !== 'macro_convention') {
    return null;
  }
  return optionId === 'standard' ? 'klipper-standard' : optionId === 'klippain' ? 'klippain' : null;
}
