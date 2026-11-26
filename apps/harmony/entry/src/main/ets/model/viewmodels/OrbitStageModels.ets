import {
  DiffFileDto,
  HighlightedLineDto,
  InlineSpanDto,
  PromptBadgeDto,
  QuestionItemDto,
  RichBlockDto,
  StageTurnDto,
  TableCellDto,
  TableRowDto,
  TimelineItemDto,
  ToolCallDetailDto,
  ToolStepDescriptorDto,
  TurnCompactionItemDto,
  TurnFileDiffSummaryDto,
  TurnLiveSegmentDto
} from '../OrbitTypes';

function fnv1aUpdate(hash: number, str: string): number {
  let h = hash >>> 0;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h;
}

function fnv1aSep(hash: number): number {
  let h = (hash ^ 0x1f) >>> 0;
  h = Math.imul(h, 16777619) >>> 0;
  return h;
}

function resolveSpanKind(dto: InlineSpanDto): string {
  if (dto.link_url !== null && dto.link_url.length > 0) {
    return 'link';
  }
  if (dto.inline_code) {
    return 'code';
  }
  if (dto.bold) {
    return 'strong';
  }
  if (dto.italic) {
    return 'em';
  }
  if (dto.strikethrough) {
    return 'strikethrough';
  }
  return 'text';
}

function normalizeTableHeaders(rawHeaders: (string | TableCellDto)[] | undefined): TableCellDto[] {
  if (rawHeaders === undefined || rawHeaders.length === 0) {
    return [];
  }
  const out: TableCellDto[] = [];
  for (let i = 0; i < rawHeaders.length; i++) {
    const h = rawHeaders[i];
    if (typeof h === 'string') {
      out.push({
        spans: [{
          text: h,
          bold: false,
          italic: false,
          strikethrough: false,
          inline_code: false,
          link_url: null
        }]
      });
    } else {
      out.push(h);
    }
  }
  return out;
}

function normalizeTableRows(rawRows: (string[] | TableRowDto)[] | undefined): TableRowDto[] {
  if (rawRows === undefined || rawRows.length === 0) {
    return [];
  }
  const out: TableRowDto[] = [];
  for (let i = 0; i < rawRows.length; i++) {
    const r = rawRows[i];
    if (Array.isArray(r)) {
      const cells: TableCellDto[] = [];
      for (let c = 0; c < r.length; c++) {
        cells.push({
          spans: [{
            text: r[c],
            bold: false,
            italic: false,
            strikethrough: false,
            inline_code: false,
            link_url: null
          }]
        });
      }
      out.push({ cells: cells });
    } else {
      out.push(r);
    }
  }
  return out;
}

/**
 * Deterministic content signature computed strictly inside `patchFromDto` (never in `build()`).
 */
export function computeRichBlockContentSig(dto: RichBlockDto): string {
  let h = 2166136261 >>> 0;
  h = fnv1aUpdate(h, dto.kind);
  h = fnv1aSep(h);
  h = fnv1aUpdate(h, `${dto.level ?? 0}`);
  h = fnv1aSep(h);
  if (dto.language !== undefined) {
    h = fnv1aUpdate(h, dto.language);
  }
  h = fnv1aSep(h);
  if (dto.raw_code !== undefined) {
    h = fnv1aUpdate(h, dto.raw_code);
  }
  h = fnv1aSep(h);
  h = fnv1aUpdate(h, `${dto.ordered === true ? 1 : 0}:${dto.index ?? 0}:${dto.checked === true ? 1 : (dto.checked === false ? 0 : -1)}`);
  h = fnv1aSep(h);
  const spans = dto.spans ?? [];
  h = fnv1aUpdate(h, `s${spans.length}`);
  for (let i = 0; i < spans.length; i++) {
    const sp = spans[i];
    h = fnv1aSep(h);
    h = fnv1aUpdate(h, sp.text);
    h = fnv1aUpdate(
      h,
      `${sp.bold ? 1 : 0}${sp.italic ? 1 : 0}${sp.strikethrough ? 1 : 0}${sp.inline_code ? 1 : 0}${sp.link_url ?? ''}`
    );
  }
  const lines = dto.lines ?? [];
  if (lines.length > 0) {
    h = fnv1aSep(h);
    h = fnv1aUpdate(h, `l${lines.length}`);
    for (let i = 0; i < lines.length; i++) {
      const ln = lines[i];
      h = fnv1aUpdate(h, `${ln.line_number}:${ln.tokens.length}`);
      for (let t = 0; t < ln.tokens.length; t++) {
        h = fnv1aUpdate(h, ln.tokens[t].text);
      }
    }
  }
  const headers = dto.headers ?? [];
  if (headers.length > 0) {
    h = fnv1aSep(h);
    h = fnv1aUpdate(h, `h${headers.length}`);
    for (let i = 0; i < headers.length; i++) {
      const hd = headers[i];
      if (typeof hd === 'string') {
        h = fnv1aUpdate(h, hd);
      } else {
        for (let j = 0; j < hd.spans.length; j++) {
          h = fnv1aUpdate(h, hd.spans[j].text);
        }
      }
    }
  }
  const rows = dto.rows ?? [];
  if (rows.length > 0) {
    h = fnv1aSep(h);
    h = fnv1aUpdate(h, `r${rows.length}`);
    for (let i = 0; i < rows.length; i++) {
      const row = rows[i];
      if (Array.isArray(row)) {
        for (let c = 0; c < row.length; c++) {
          h = fnv1aUpdate(h, row[c]);
        }
      } else {
        for (let c = 0; c < row.cells.length; c++) {
          const cellSpans = row.cells[c].spans;
          for (let j = 0; j < cellSpans.length; j++) {
            h = fnv1aUpdate(h, cellSpans[j].text);
          }
        }
      }
    }
  }
  return `${dto.kind}:${h.toString(16)}`;
}

export function deriveTimelineItemKey(dto: TimelineItemDto): string {
  if (dto.item_id !== undefined && dto.item_id.length > 0) {
    return `${dto.item_type}:${dto.item_id}`;
  }
  if (dto.call_id !== undefined && dto.call_id.length > 0) {
    return `${dto.item_type}:${dto.call_id}`;
  }
  if (dto.message_id !== undefined && dto.message_id.length > 0) {
    return `${dto.item_type}:${dto.message_id}`;
  }
  if (dto.request_id !== undefined && dto.request_id.length > 0) {
    return `${dto.item_type}:${dto.request_id}`;
  }
  return `${dto.item_type}:${dto.seq}`;
}

export function deriveRichBlockKey(dto: RichBlockDto, index: number): string {
  return `${dto.kind}#${index}`;
}

@ObservedV2
export class OrbitInlineSpanModel {
  readonly spanKey: string;
  @Trace kind: string = '';
  @Trace text: string = '';
  @Trace href: string = '';
  @Trace mathMl: string = '';
  @Trace mathUnicode: string = '';
  @Trace bold: boolean = false;
  @Trace italic: boolean = false;
  @Trace strikethrough: boolean = false;
  @Trace inlineCode: boolean = false;

  constructor(spanKey: string, dto?: InlineSpanDto) {
    this.spanKey = spanKey;
    if (dto !== undefined) {
      this.patchFromDto(dto);
    }
  }

  public patchFromDto(dto: InlineSpanDto): void {
    const nextKind = resolveSpanKind(dto);
    if (this.kind !== nextKind) {
      this.kind = nextKind;
    }
    if (this.text !== dto.text) {
      this.text = dto.text;
    }
    const nextHref = dto.link_url ?? '';
    if (this.href !== nextHref) {
      this.href = nextHref;
    }
    if (this.bold !== dto.bold) {
      this.bold = dto.bold;
    }
    if (this.italic !== dto.italic) {
      this.italic = dto.italic;
    }
    if (this.strikethrough !== dto.strikethrough) {
      this.strikethrough = dto.strikethrough;
    }
    if (this.inlineCode !== dto.inline_code) {
      this.inlineCode = dto.inline_code;
    }
  }
}

@ObservedV2
export class OrbitRichBlockModel {
  readonly blockKey: string;
  @Trace kind: string = '';
  @Trace level: number = 0;
  @Trace spans: OrbitInlineSpanModel[] = [];
  @Trace language: string = '';
  @Trace rawCode: string = '';
  @Trace codeLines: HighlightedLineDto[] = [];
  @Trace diagramKind: string = '';
  @Trace ordered: boolean = false;
  @Trace listIndex: number = 0;
  @Trace tableAlignments: string[] = [];
  @Trace tableHeaders: TableCellDto[] = [];
  @Trace tableRows: TableRowDto[] = [];
  @Trace mathTex: string = '';
  @Trace mathMl: string = '';
  @Trace mathUnicode: string = '';
  @Trace checked: boolean | null = null;
  contentSig: string = '';
  rawDto: RichBlockDto;

  constructor(blockKey: string, dto: RichBlockDto) {
    this.blockKey = blockKey;
    this.rawDto = dto;
    this.patchFromDto(dto);
  }

  public patchFromDto(dto: RichBlockDto): void {
    const nextSig = computeRichBlockContentSig(dto);
    if (nextSig === this.contentSig && this.contentSig.length > 0) {
      return;
    }
    this.contentSig = nextSig;
    this.rawDto = dto;

    if (this.kind !== dto.kind) {
      this.kind = dto.kind;
    }
    const nextLevel = dto.level ?? 0;
    if (this.level !== nextLevel) {
      this.level = nextLevel;
    }
    const nextLang = dto.language ?? '';
    if (this.language !== nextLang) {
      this.language = nextLang;
    }
    const nextCode = dto.raw_code ?? '';
    if (this.rawCode !== nextCode) {
      this.rawCode = nextCode;
    }
    this.codeLines = dto.lines ?? [];
    const nextOrdered = dto.ordered === true;
    if (this.ordered !== nextOrdered) {
      this.ordered = nextOrdered;
    }
    const nextIndex = dto.index ?? 0;
    if (this.listIndex !== nextIndex) {
      this.listIndex = nextIndex;
    }
    const nextChecked = dto.checked !== undefined ? dto.checked : null;
    if (this.checked !== nextChecked) {
      this.checked = nextChecked;
    }
    this.tableAlignments = dto.alignments ?? [];
    this.tableHeaders = normalizeTableHeaders(dto.headers);
    this.tableRows = normalizeTableRows(dto.rows);

    const incomingSpans = dto.spans ?? [];
    for (let i = 0; i < incomingSpans.length; i++) {
      const spDto = incomingSpans[i];
      if (i < this.spans.length) {
        this.spans[i].patchFromDto(spDto);
      } else {
        this.spans.push(new OrbitInlineSpanModel(`${this.blockKey}:sp#${i}`, spDto));
      }
    }
    if (this.spans.length > incomingSpans.length) {
      this.spans.splice(incomingSpans.length, this.spans.length - incomingSpans.length);
    }
  }
}

@ObservedV2
export class OrbitCompactionModel {
  readonly compactionKey: string;
  @Trace seq: number = 0;
  @Trace label: string = '';

  constructor(dto: TurnCompactionItemDto) {
    this.compactionKey = `compaction:${dto.seq}`;
    this.patchFromDto(dto);
  }

  public patchFromDto(dto: TurnCompactionItemDto): void {
    if (this.seq !== dto.seq) {
      this.seq = dto.seq;
    }
    if (this.label !== dto.label) {
      this.label = dto.label;
    }
  }
}

@ObservedV2
export class OrbitTimelineItemModel {
  readonly itemKey: string;
  @Trace itemType: string = '';
  @Trace seq: number = 0;
  @Trace status: string = '';
  @Trace title: string = '';
  @Trace detailExpanded: boolean = false;
  @Trace blocks: OrbitRichBlockModel[] = [];
  @Trace rawBlocks: RichBlockDto[] = [];
  @Trace rawText: string = '';
  @Trace isAgentMessage: boolean = false;
  @Trace agentVisibility: string = '';
  @Trace sourceAgentId: string = '';
  @Trace sourceHostId: string = '';
  @Trace sourceTitle: string = '';
  @Trace deliveryStatus: string = '';
  @Trace startedAtMs: number = 0;
  @Trace durationMs: number = 0;
  @Trace callId: string = '';
  @Trace requestId: string = '';
  @Trace streaming: boolean = false;
  @Trace level: string = '';
  @Trace message: string = '';
  @Trace kind: string = '';
  @Trace badges: PromptBadgeDto[] = [];
  @Trace detail: ToolCallDetailDto | null = null;
  @Trace stepDescriptor: ToolStepDescriptorDto | null = null;
  @Trace diffFiles: DiffFileDto[] = [];
  @Trace allowedDecisions: string[] = [];
  @Trace questions: QuestionItemDto[] = [];
  rawDto: TimelineItemDto;

  private blockByKey: Map<string, OrbitRichBlockModel> = new Map<string, OrbitRichBlockModel>();
  private blocksCompositeSig: string = '';

  constructor(dto: TimelineItemDto) {
    this.itemKey = deriveTimelineItemKey(dto);
    this.rawDto = dto;
    this.patchFromDto(dto);
  }

  public patchFromDto(dto: TimelineItemDto): void {
    this.rawDto = dto;

    if (this.itemType !== dto.item_type) {
      this.itemType = dto.item_type;
    }
    if (this.seq !== dto.seq) {
      this.seq = dto.seq;
    }
    const nextStatus = dto.status ?? '';
    if (this.status !== nextStatus) {
      this.status = nextStatus;
    }
    const nextTitle = dto.title ?? dto.summary_title ?? '';
    if (this.title !== nextTitle) {
      this.title = nextTitle;
    }
    const nextRawText = dto.text ?? dto.message ?? '';
    if (this.rawText !== nextRawText) {
      this.rawText = nextRawText;
    }
    const nextIsAgentMsg = dto.item_type === 'agent_message';
    if (this.isAgentMessage !== nextIsAgentMsg) {
      this.isAgentMessage = nextIsAgentMsg;
    }
    const nextSourceAgentId = dto.source_agent_id ?? '';
    if (this.sourceAgentId !== nextSourceAgentId) {
      this.sourceAgentId = nextSourceAgentId;
    }
    const nextSourceHostId = dto.source_host_id ?? '';
    if (this.sourceHostId !== nextSourceHostId) {
      this.sourceHostId = nextSourceHostId;
    }
    const nextSourceTitle = dto.source_title ?? '';
    if (this.sourceTitle !== nextSourceTitle) {
      this.sourceTitle = nextSourceTitle;
    }
    const nextDeliveryStatus = dto.delivery_status ?? '';
    if (this.deliveryStatus !== nextDeliveryStatus) {
      this.deliveryStatus = nextDeliveryStatus;
    }
    const nextStartedAtMs = dto.timestamp_ms ?? 0;
    if (this.startedAtMs !== nextStartedAtMs) {
      this.startedAtMs = nextStartedAtMs;
    }
    const nextCallId = dto.call_id ?? '';
    if (this.callId !== nextCallId) {
      this.callId = nextCallId;
    }
    const nextRequestId = dto.request_id ?? '';
    if (this.requestId !== nextRequestId) {
      this.requestId = nextRequestId;
    }
    const nextStreaming = dto.streaming === true;
    if (this.streaming !== nextStreaming) {
      this.streaming = nextStreaming;
    }
    const nextLevel = dto.level ?? '';
    if (this.level !== nextLevel) {
      this.level = nextLevel;
    }
    const nextMessage = dto.message ?? '';
    if (this.message !== nextMessage) {
      this.message = nextMessage;
    }
    const nextKind = dto.kind ?? '';
    if (this.kind !== nextKind) {
      this.kind = nextKind;
    }
    if (dto.badges !== undefined) {
      if (this.badges.length !== dto.badges.length || dto.badges.length > 0) {
        this.badges = dto.badges;
      }
    } else if (this.badges.length > 0) {
      this.badges = [];
    }
    const nextDetail = dto.detail ?? null;
    if (this.detail !== nextDetail) {
      this.detail = nextDetail;
    }
    const nextDesc = dto.step_descriptor ?? null;
    if (this.stepDescriptor !== nextDesc) {
      this.stepDescriptor = nextDesc;
    }
    const nextDiffFiles = dto.diff_files ?? [];
    if (this.diffFiles.length !== nextDiffFiles.length || nextDiffFiles.length > 0) {
      this.diffFiles = nextDiffFiles;
    }
    const nextAllowed = dto.allowed_decisions ?? [];
    if (this.allowedDecisions.length !== nextAllowed.length || nextAllowed.length > 0) {
      this.allowedDecisions = nextAllowed;
    }
    const nextQuestions = dto.questions ?? [];
    if (this.questions.length !== nextQuestions.length || nextQuestions.length > 0) {
      this.questions = nextQuestions;
    }

    this.patchBlocks(dto.blocks ?? []);
  }

  private patchBlocks(incoming: RichBlockDto[]): void {
    const nextKeys: Set<string> = new Set<string>();
    let sigParts = '';
    for (let i = 0; i < incoming.length; i++) {
      const blkDto = incoming[i];
      const key = deriveRichBlockKey(blkDto, i);
      nextKeys.add(key);
      let model = this.blockByKey.get(key);
      if (model !== undefined) {
        model.patchFromDto(blkDto);
      } else {
        model = new OrbitRichBlockModel(key, blkDto);
        this.blockByKey.set(key, model);
      }
      if (i < this.blocks.length) {
        if (this.blocks[i] !== model) {
          this.blocks[i] = model;
        }
      } else {
        this.blocks.push(model);
      }
      sigParts += `${model.contentSig}|`;
    }
    if (this.blocks.length > incoming.length) {
      this.blocks.splice(incoming.length, this.blocks.length - incoming.length);
    }
    const staleKeys: string[] = [];
    this.blockByKey.forEach((_val: OrbitRichBlockModel, k: string): void => {
      if (!nextKeys.has(k)) {
        staleKeys.push(k);
      }
    });
    for (let i = 0; i < staleKeys.length; i++) {
      this.blockByKey.delete(staleKeys[i]);
    }
    if (sigParts !== this.blocksCompositeSig) {
      this.blocksCompositeSig = sigParts;
      this.rawBlocks = incoming;
    }
  }
}

@ObservedV2
export class OrbitLiveSegmentModel {
  readonly segKey: string;
  @Trace kind: string = '';
  @Trace items: OrbitTimelineItemModel[] = [];
  @Trace item: OrbitTimelineItemModel | null = null;
  @Trace summary: string = '';
  @Trace liveSummaryLabel: string = '';
  @Trace summaryIncludesFailure: boolean = false;
  @Trace label: string = '';
  @Trace isRunning: boolean = false;
  @Trace hasFollowingContent: boolean = false;
  @Trace stepsExpanded: boolean = false;
  @Trace earlierStepsRevealed: boolean = false;

  private itemByKey: Map<string, OrbitTimelineItemModel> = new Map<string, OrbitTimelineItemModel>();

  constructor(dto: TurnLiveSegmentDto) {
    // Strictly equal to core segment key — never concatenate items.length or running flags.
    this.segKey = dto.key;
    this.patchFromDto(dto);
  }

  public patchFromDto(dto: TurnLiveSegmentDto): void {
    if (this.kind !== dto.kind) {
      this.kind = dto.kind;
    }
    const nextSummary = dto.summary?.label ?? '';
    if (this.summary !== nextSummary) {
      this.summary = nextSummary;
    }
    const nextLiveSummary = dto.summary?.live_label ?? '';
    if (this.liveSummaryLabel !== nextLiveSummary) {
      this.liveSummaryLabel = nextLiveSummary;
    }
    const nextIncludesFailure = dto.summary?.label_includes_failure === true;
    if (this.summaryIncludesFailure !== nextIncludesFailure) {
      this.summaryIncludesFailure = nextIncludesFailure;
    }
    const nextLabel = dto.label ?? '';
    if (this.label !== nextLabel) {
      this.label = nextLabel;
    }
    const nextRunning = dto.has_running_step === true;
    if (this.isRunning !== nextRunning) {
      this.isRunning = nextRunning;
    }
    const nextFollowing = dto.has_following_content === true;
    if (this.hasFollowingContent !== nextFollowing) {
      this.hasFollowingContent = nextFollowing;
    }

    if (dto.item !== undefined) {
      const incomingKey = deriveTimelineItemKey(dto.item);
      if (this.item !== null && this.item.itemKey === incomingKey) {
        this.item.patchFromDto(dto.item);
      } else {
        this.item = new OrbitTimelineItemModel(dto.item);
      }
    } else if (this.item !== null) {
      this.item = null;
    }

    const incomingItems = dto.items ?? [];
    const nextKeys: Set<string> = new Set<string>();
    for (let i = 0; i < incomingItems.length; i++) {
      const itemDto = incomingItems[i];
      const key = deriveTimelineItemKey(itemDto);
      nextKeys.add(key);
      let model = this.itemByKey.get(key);
      if (model !== undefined) {
        model.patchFromDto(itemDto);
      } else {
        model = new OrbitTimelineItemModel(itemDto);
        this.itemByKey.set(key, model);
      }
      if (i < this.items.length) {
        if (this.items[i] !== model) {
          this.items[i] = model;
        }
      } else {
        this.items.push(model);
      }
    }
    if (this.items.length > incomingItems.length) {
      this.items.splice(incomingItems.length, this.items.length - incomingItems.length);
    }
    const staleKeys: string[] = [];
    this.itemByKey.forEach((_val: OrbitTimelineItemModel, k: string): void => {
      if (!nextKeys.has(k)) {
        staleKeys.push(k);
      }
    });
    for (let i = 0; i < staleKeys.length; i++) {
      this.itemByKey.delete(staleKeys[i]);
    }
  }
}

function turnDiffsEqual(a: TurnFileDiffSummaryDto[], b: TurnFileDiffSummaryDto[]): boolean {
  if (a.length !== b.length) {
    return false;
  }
  for (let i = 0; i < a.length; i++) {
    if (
      a[i].path !== b[i].path ||
      a[i].additions !== b[i].additions ||
      a[i].deletions !== b[i].deletions
    ) {
      return false;
    }
  }
  return true;
}

@ObservedV2
export class OrbitStageTurnModel {
  readonly turnKey: string;
  @Trace turnIndex: number = 0;
  @Trace anchorSeq: number = 0;
  @Trace workOrdinal: number = 0;
  @Trace isCompactionCommand: boolean = false;
  @Trace userPrompt: OrbitTimelineItemModel | null = null;
  @Trace agentMessage: OrbitTimelineItemModel | null = null;
  @Trace foldedSteps: OrbitTimelineItemModel[] = [];
  @Trace liveSegments: OrbitLiveSegmentModel[] = [];
  @Trace terminalReply: OrbitTimelineItemModel | null = null;
  @Trace notices: OrbitTimelineItemModel[] = [];
  @Trace compactions: OrbitCompactionModel[] = [];
  @Trace turnDiffs: TurnFileDiffSummaryDto[] = [];
  @Trace workSummaryLabel: string = '';
  @Trace workSummaryLiveLabel: string = '';
  @Trace workSummaryLiveDetail: string = '';
  @Trace workSummaryIncludesFailure: boolean = false;
  @Trace thoughtDurationMs: number = 0;
  @Trace workDurationMs: number = 0;
  @Trace turnDurationMs: number = 0;
  @Trace hasError: boolean = false;
  @Trace hasUnrecoveredError: boolean = false;
  @Trace hasActiveItem: boolean = false;
  @Trace startedAtMs: number = 0;
  @Trace completedAtMs: number = 0;
  @Trace latestActivityMs: number = 0;
  @Trace latestVisibleProgressMs: number = 0;
  @Trace stepsExpanded: boolean = false;
  @Trace earlierStepsRevealed: boolean = false;
  @Trace changesExpanded: boolean = false;

  private foldedByKey: Map<string, OrbitTimelineItemModel> = new Map<string, OrbitTimelineItemModel>();
  private segmentByKey: Map<string, OrbitLiveSegmentModel> = new Map<string, OrbitLiveSegmentModel>();
  private noticeByKey: Map<string, OrbitTimelineItemModel> = new Map<string, OrbitTimelineItemModel>();
  private compactionByKey: Map<string, OrbitCompactionModel> = new Map<string, OrbitCompactionModel>();

  constructor(dto?: StageTurnDto) {
    this.turnKey = dto !== undefined ? dto.key : '';
    if (dto !== undefined) {
      this.patchFromDto(dto);
    }
  }

  public patchFromDto(dto: StageTurnDto): void {
    if (this.turnIndex !== dto.turn_index) {
      this.turnIndex = dto.turn_index;
    }
    if (this.anchorSeq !== dto.anchor_seq) {
      this.anchorSeq = dto.anchor_seq;
    }
    if (this.isCompactionCommand !== dto.is_compaction_command) {
      this.isCompactionCommand = dto.is_compaction_command;
    }
    if (this.workSummaryLabel !== dto.work_summary_label) {
      this.workSummaryLabel = dto.work_summary_label;
    }
    if (this.workSummaryLiveLabel !== dto.work_summary_live_label) {
      this.workSummaryLiveLabel = dto.work_summary_live_label;
    }
    const nextLiveDetail = dto.work_summary_live_detail ?? '';
    if (this.workSummaryLiveDetail !== nextLiveDetail) {
      this.workSummaryLiveDetail = nextLiveDetail;
    }
    if (this.workSummaryIncludesFailure !== dto.work_summary_includes_failure) {
      this.workSummaryIncludesFailure = dto.work_summary_includes_failure;
    }
    const nextThoughtMs = dto.thought_duration_ms ?? 0;
    if (this.thoughtDurationMs !== nextThoughtMs) {
      this.thoughtDurationMs = nextThoughtMs;
    }
    const nextWorkMs = dto.work_duration_ms ?? 0;
    if (this.workDurationMs !== nextWorkMs) {
      this.workDurationMs = nextWorkMs;
    }
    const nextTurnMs = dto.turn_duration_ms ?? 0;
    if (this.turnDurationMs !== nextTurnMs) {
      this.turnDurationMs = nextTurnMs;
    }
    if (this.hasError !== dto.has_error) {
      this.hasError = dto.has_error;
    }
    if (this.hasUnrecoveredError !== dto.has_unrecovered_error) {
      this.hasUnrecoveredError = dto.has_unrecovered_error;
    }
    if (this.hasActiveItem !== dto.has_active_item) {
      this.hasActiveItem = dto.has_active_item;
    }
    const nextStarted = dto.started_at_ms ?? 0;
    if (this.startedAtMs !== nextStarted) {
      this.startedAtMs = nextStarted;
    }
    const nextCompleted = dto.completed_at_ms ?? 0;
    if (this.completedAtMs !== nextCompleted) {
      this.completedAtMs = nextCompleted;
    }
    const nextLatestAct = dto.latest_activity_ms ?? 0;
    if (this.latestActivityMs !== nextLatestAct) {
      this.latestActivityMs = nextLatestAct;
    }
    const nextLatestVis = dto.latest_visible_progress_ms ?? 0;
    if (this.latestVisibleProgressMs !== nextLatestVis) {
      this.latestVisibleProgressMs = nextLatestVis;
    }

    if (dto.user_prompt !== null) {
      const pKey = deriveTimelineItemKey(dto.user_prompt);
      if (this.userPrompt !== null && this.userPrompt.itemKey === pKey) {
        this.userPrompt.patchFromDto(dto.user_prompt);
      } else {
        this.userPrompt = new OrbitTimelineItemModel(dto.user_prompt);
      }
    } else if (this.userPrompt !== null) {
      this.userPrompt = null;
    }

    if (dto.agent_message !== undefined && dto.agent_message !== null) {
      const amKey = deriveTimelineItemKey(dto.agent_message);
      if (this.agentMessage !== null && this.agentMessage.itemKey === amKey) {
        this.agentMessage.patchFromDto(dto.agent_message);
      } else {
        this.agentMessage = new OrbitTimelineItemModel(dto.agent_message);
      }
    } else if (this.agentMessage !== null) {
      this.agentMessage = null;
    }

    if (dto.terminal_reply !== null) {
      const trKey = deriveTimelineItemKey(dto.terminal_reply);
      if (this.terminalReply !== null && this.terminalReply.itemKey === trKey) {
        this.terminalReply.patchFromDto(dto.terminal_reply);
      } else {
        this.terminalReply = new OrbitTimelineItemModel(dto.terminal_reply);
      }
    } else if (this.terminalReply !== null) {
      this.terminalReply = null;
    }

    this.patchTimelineItemList(this.foldedSteps, this.foldedByKey, dto.folded_steps);
    this.patchLiveSegments(dto.live_segments);
    this.patchTimelineItemList(this.notices, this.noticeByKey, dto.notices);
    this.patchCompactions(dto.compactions ?? []);

    if (!turnDiffsEqual(this.turnDiffs, dto.turn_diffs)) {
      this.turnDiffs = dto.turn_diffs;
    }
  }

  private patchTimelineItemList(
    target: OrbitTimelineItemModel[],
    indexMap: Map<string, OrbitTimelineItemModel>,
    incoming: TimelineItemDto[]
  ): void {
    const nextKeys: Set<string> = new Set<string>();
    for (let i = 0; i < incoming.length; i++) {
      const itemDto = incoming[i];
      const key = deriveTimelineItemKey(itemDto);
      nextKeys.add(key);
      let model = indexMap.get(key);
      if (model !== undefined) {
        model.patchFromDto(itemDto);
      } else {
        model = new OrbitTimelineItemModel(itemDto);
        indexMap.set(key, model);
      }
      if (i < target.length) {
        if (target[i] !== model) {
          target[i] = model;
        }
      } else {
        target.push(model);
      }
    }
    if (target.length > incoming.length) {
      target.splice(incoming.length, target.length - incoming.length);
    }
    const staleKeys: string[] = [];
    indexMap.forEach((_v: OrbitTimelineItemModel, k: string): void => {
      if (!nextKeys.has(k)) {
        staleKeys.push(k);
      }
    });
    for (let i = 0; i < staleKeys.length; i++) {
      indexMap.delete(staleKeys[i]);
    }
  }

  private patchLiveSegments(incoming: TurnLiveSegmentDto[]): void {
    const nextKeys: Set<string> = new Set<string>();
    let workGroupCount = 0;
    for (let i = 0; i < incoming.length; i++) {
      const segDto = incoming[i];
      if (segDto.kind === 'step_group') {
        workGroupCount += 1;
      }
      const key = segDto.key;
      nextKeys.add(key);
      let model = this.segmentByKey.get(key);
      if (model !== undefined) {
        model.patchFromDto(segDto);
      } else {
        model = new OrbitLiveSegmentModel(segDto);
        this.segmentByKey.set(key, model);
      }
      if (i < this.liveSegments.length) {
        if (this.liveSegments[i] !== model) {
          this.liveSegments[i] = model;
        }
      } else {
        this.liveSegments.push(model);
      }
    }
    if (this.liveSegments.length > incoming.length) {
      this.liveSegments.splice(incoming.length, this.liveSegments.length - incoming.length);
    }
    const staleKeys: string[] = [];
    this.segmentByKey.forEach((_v: OrbitLiveSegmentModel, k: string): void => {
      if (!nextKeys.has(k)) {
        staleKeys.push(k);
      }
    });
    for (let i = 0; i < staleKeys.length; i++) {
      this.segmentByKey.delete(staleKeys[i]);
    }
    if (this.workOrdinal !== workGroupCount) {
      this.workOrdinal = workGroupCount;
    }
  }

  private patchCompactions(incoming: TurnCompactionItemDto[]): void {
    const nextKeys: Set<string> = new Set<string>();
    for (let i = 0; i < incoming.length; i++) {
      const compDto = incoming[i];
      const key = `compaction:${compDto.seq}`;
      nextKeys.add(key);
      let model = this.compactionByKey.get(key);
      if (model !== undefined) {
        model.patchFromDto(compDto);
      } else {
        model = new OrbitCompactionModel(compDto);
        this.compactionByKey.set(key, model);
      }
      if (i < this.compactions.length) {
        if (this.compactions[i] !== model) {
          this.compactions[i] = model;
        }
      } else {
        this.compactions.push(model);
      }
    }
    if (this.compactions.length > incoming.length) {
      this.compactions.splice(incoming.length, this.compactions.length - incoming.length);
    }
    const staleKeys: string[] = [];
    this.compactionByKey.forEach((_v: OrbitCompactionModel, k: string): void => {
      if (!nextKeys.has(k)) {
        staleKeys.push(k);
      }
    });
    for (let i = 0; i < staleKeys.length; i++) {
      this.compactionByKey.delete(staleKeys[i]);
    }
  }
}
