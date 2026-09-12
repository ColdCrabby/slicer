import type { DetectionOption, DetectionQuestion } from '../../services/printer-connection';

/**
 * Wording for the setup questions detection raises, and the profile-level
 * effects the engine cannot express.
 *
 * The engine reports what a printer's config does and does not settle; it has
 * no opinion on how to phrase that. So the machine-specific half — which
 * options exist, which one the evidence points at — arrives on the
 * {@link DetectionQuestion}, and the plain-language half lives here, keyed by
 * the same ids.
 *
 * Options whose effect is a slicing parameter carry it in `option.params` and
 * need no entry beyond a label. The ones listed in
 * {@link PROFILE_EFFECTS} change a *profile* field instead — a vendor, a model,
 * a preferred plate orientation — which is not something a params bag can
 * express, so the wizard applies those by id.
 */

/** Wording for one question and its known options. */
export interface QuestionCopy {
  /** Step label in the wizard's progress strip. Two words or so. */
  readonly step: string;
  /** The question itself, asked plainly. */
  readonly headline: string;
  /** Optional sentence under the headline. */
  readonly detail?: string;
  /** Labels and descriptions for options the engine cannot name. */
  readonly options: Readonly<Record<string, { label: string; description?: string }>>;
  /**
   * Phrasing for options the engine *did* name — a fan object, a saved mesh
   * profile. The bare name is data, not an answer: "rscs" next to "Not for
   * cooling prints" reads as a riddle, "Use rscs for cooling" does not.
   */
  readonly nameOption?: (name: string) => string;
  /** Link out for a term the question cannot avoid using. */
  readonly learnMore?: { readonly label: string; readonly url: string };
}

const KLIPPAIN_README_URL = 'https://github.com/Frix-x/klippain/blob/main/README.md';

const QUESTION_COPY: Readonly<Record<string, QuestionCopy>> = {
  machine_identity: {
    step: 'Your printer',
    headline: 'Is this your printer?',
    detail: 'We matched its configuration against machines we know.',
    options: {
      other: {
        label: 'Something else',
        description: "A custom build, or a machine we don't recognise. You can name it yourself.",
      },
    },
  },
  macro_convention: {
    step: 'Start macros',
    headline: 'How should a print start and end?',
    detail: 'This decides the commands we put at the top and bottom of every file.',
    options: {
      standard: {
        label: 'PRINT_START / PRINT_END',
        description: 'The mainline Klipper convention.',
      },
      klippain: {
        label: 'START_PRINT / END_PRINT',
        description: 'Klippain, which also takes chamber and material.',
      },
      keep: {
        label: 'Leave it to me',
        description: "We'll write nothing, and you can paste in your own.",
      },
    },
    learnMore: { label: "What's Klippain?", url: KLIPPAIN_README_URL },
  },
  bed_mesh: {
    step: 'Bed levelling',
    headline: 'Should we level the bed before each print?',
    options: {
      leave: {
        label: 'My start macro does it',
        description: 'Most start macros already probe. Safest, and adds no time.',
      },
      calibrate: {
        label: 'Probe every print',
        description: 'Re-measures only the area the print actually covers.',
      },
    },
    nameOption: (name) => `Load ${name}`,
  },
  aux_fan: {
    step: 'Extra fan',
    headline: 'What is this fan for?',
    detail: "Klipper can't tell us — it could be cooling, filtration or an electronics bay.",
    options: {
      unused: {
        label: 'Not for cooling prints',
        description: "We won't touch it.",
      },
    },
    nameOption: (name) => `Cool prints with ${name}`,
  },
  preferred_orientation: {
    step: 'Orientation',
    headline: 'Print parts rotated 45°?',
    detail: 'A CoreXY moves fastest along its diagonals, so turning parts can print quicker.',
    options: {
      keep: { label: 'No, place parts as they come', description: 'The usual choice.' },
      diagonal: { label: 'Yes, rotate 45°', description: 'Keeps long walls off the belt axes.' },
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

/** Wording for a question id, or `null` for one this build doesn't know. */
export function questionCopy(id: string): QuestionCopy | null {
  return QUESTION_COPY[id] ?? null;
}

/**
 * Label for an option: the engine's own when it named it (a fan object, a saved
 * mesh profile), otherwise this build's copy, otherwise the bare id.
 */
export function optionLabel(question: DetectionQuestion, option: DetectionOption): string {
  const copy = questionCopy(question.id);
  if (option.label) {
    return copy?.nameOption?.(option.label) ?? option.label;
  }
  return copy?.options[option.id]?.label ?? option.id;
}

/** One-line description for an option, when there is one worth showing. */
export function optionDescription(
  question: DetectionQuestion,
  option: DetectionOption,
): string | undefined {
  return option.detail ?? questionCopy(question.id)?.options[option.id]?.description;
}

/** Profile-field patch an answer implies, if any. */
export function optionProfilePatch(
  questionId: string,
  optionId: string,
): { preferred_orientation_deg?: number } | null {
  return PROFILE_EFFECTS[questionId]?.[optionId] ?? null;
}

/**
 * G-code template id an answer selects, or `null` when the answer is "leave my
 * macros alone" or the question isn't about macros.
 *
 * Template *copy* stays in `gcode-templates.ts`; the engine only ever reports
 * which macros a printer defines.
 */
export function optionTemplateId(questionId: string, optionId: string): string | null {
  if (questionId !== 'macro_convention') {
    return null;
  }
  return optionId === 'standard' ? 'klipper-standard' : optionId === 'klippain' ? 'klippain' : null;
}
