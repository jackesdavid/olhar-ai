// The home-screen widget (card #178): one button that runs a workflow you choose.
//
// Configurable per instance, the iOS way — long-press → Edit Widget → pick the workflow. That
// matters more than it sounds: a widget you configure is one you can place several of, each opening
// a different thing, named the way you named it. The first version made you star workflows inside
// the app instead, which is the app dictating the home screen rather than the other way round.
//
// A widget extension is a separate process with its own sandbox. It holds NO credentials and speaks
// to no API — the button is a `Link` to `olharai://run/<id>`, which the app already handles. A
// widget that called the control plane itself would be a second implementation of "run a workflow",
// with its own auth and its own bugs, and would need a session token living in an extension.
//
// Everything it knows comes through the shared App Group container, written by the app.
import AppIntents
import SwiftUI
import WidgetKit

/// One workflow, as the app publishes it into the App Group.
struct Workflow: Codable, Identifiable, Hashable {
  let id: String
  let name: String
}

/// Reads what the app shared. Never network, never disk beyond the container.
enum SharedWorkflows {
  static let suite = "group.com.olharai.app"
  static let key = "workflows"

  static func read() -> [Workflow] {
    guard
      let defaults = UserDefaults(suiteName: suite),
      let raw = defaults.string(forKey: key),
      let data = raw.data(using: .utf8),
      let decoded = try? JSONDecoder().decode([Workflow].self, from: data)
    else { return [] }
    return decoded
  }

  static func named(_ id: String) -> Workflow? {
    read().first { $0.id == id }
  }
}

// MARK: - The picker

/// What the Edit Widget sheet offers. The entity is the workflow itself, so the sheet shows real
/// names rather than asking someone to remember a UUID.
struct WorkflowEntity: AppEntity {
  let id: String
  let name: String

  static var typeDisplayRepresentation: TypeDisplayRepresentation { "Workflow" }
  var displayRepresentation: DisplayRepresentation { DisplayRepresentation(title: "\(name)") }
  static var defaultQuery = WorkflowQuery()
}

struct WorkflowQuery: EntityQuery {
  func entities(for identifiers: [String]) async throws -> [WorkflowEntity] {
    SharedWorkflows.read()
      .filter { identifiers.contains($0.id) }
      .map { WorkflowEntity(id: $0.id, name: $0.name) }
  }

  /// The list in the sheet. Empty until the app has run once and published — which is why the
  /// unconfigured tile says to open the app rather than showing a blank picker.
  func suggestedEntities() async throws -> [WorkflowEntity] {
    SharedWorkflows.read().map { WorkflowEntity(id: $0.id, name: $0.name) }
  }

  func defaultResult() async -> WorkflowEntity? {
    SharedWorkflows.read().first.map { WorkflowEntity(id: $0.id, name: $0.name) }
  }
}

struct SelectWorkflowIntent: WidgetConfigurationIntent {
  static var title: LocalizedStringResource { "Choose a workflow" }
  static var description: IntentDescription { "Pick the workflow this button runs." }

  @Parameter(title: "Workflow")
  var workflow: WorkflowEntity?
}

// MARK: - Timeline

struct WorkflowEntry: TimelineEntry {
  let date: Date
  let workflow: Workflow?
  /// True when the app has never published anything — a different problem from "not configured
  /// yet", and it needs different advice.
  let appNeverOpened: Bool
}

struct Provider: AppIntentTimelineProvider {
  func placeholder(in context: Context) -> WorkflowEntry {
    WorkflowEntry(
      date: Date(),
      workflow: Workflow(id: "", name: "Open the garage"),
      appNeverOpened: false
    )
  }

  func snapshot(for configuration: SelectWorkflowIntent, in context: Context) async -> WorkflowEntry {
    entry(for: configuration)
  }

  func timeline(for configuration: SelectWorkflowIntent, in context: Context) async -> Timeline<WorkflowEntry> {
    // `.never`: this is a button, not a countdown. Nothing about it goes stale on a clock — it
    // changes when the app republishes and calls `reloadAllTimelines`, and asking iOS to refresh on
    // a schedule would spend the system's widget budget redrawing the same name.
    Timeline(entries: [entry(for: configuration)], policy: .never)
  }

  private func entry(for configuration: SelectWorkflowIntent) -> WorkflowEntry {
    let all = SharedWorkflows.read()
    guard let chosen = configuration.workflow else {
      return WorkflowEntry(date: Date(), workflow: nil, appNeverOpened: all.isEmpty)
    }
    // Prefer the CURRENT name: a workflow renamed on the web should not leave a stale label on the
    // home screen, and the configuration only remembers what it was called when it was picked.
    let live = SharedWorkflows.named(chosen.id)
    return WorkflowEntry(
      date: Date(),
      workflow: Workflow(id: chosen.id, name: live?.name ?? chosen.name),
      appNeverOpened: all.isEmpty
    )
  }
}

// MARK: - The tile

/// The Olhar AI mark, drawn where the widget can use it.
///
/// An eclipse: a filled disc with a crescent bitten out of its upper right, a small dot inside the
/// bite and a satellite further out. Traced from `assets/olhar-ai-mark-white.svg` — the same
/// geometry as the app icon and the in-app mark, so the widget is not the one surface of the
/// product that fails to look like the product. It is also the surface people see most often.
struct EyeMark: View {
  var size: CGFloat
  /// The surface behind the mark: the crescent is SUBTRACTIVE, so the bite has to be painted in
  /// whatever colour sits under it.
  var bg: Color

  // The SVG's own 120×120 space, so these numbers match the source file line for line.
  private let box: CGFloat = 120
  private let discR: CGFloat = 31
  private let bite = (x: CGFloat(71), y: CGFloat(49), r: CGFloat(27))
  private let pupil = (x: CGFloat(71), y: CGFloat(49), r: CGFloat(6))
  private let satellite = (x: CGFloat(95.4), y: CGFloat(24.6), r: CGFloat(4))

  var body: some View {
    let k = size / box
    ZStack(alignment: .topLeading) {
      Circle()
        .frame(width: discR * 2 * k, height: discR * 2 * k)
        .offset(x: (box / 2 - discR) * k, y: (box / 2 - discR) * k)
      Circle()
        .foregroundStyle(bg)
        .frame(width: bite.r * 2 * k, height: bite.r * 2 * k)
        .offset(x: (bite.x - bite.r) * k, y: (bite.y - bite.r) * k)
      Circle()
        .frame(width: pupil.r * 2 * k, height: pupil.r * 2 * k)
        .offset(x: (pupil.x - pupil.r) * k, y: (pupil.y - pupil.r) * k)
      Circle()
        .frame(width: satellite.r * 2 * k, height: satellite.r * 2 * k)
        .offset(x: (satellite.x - satellite.r) * k, y: (satellite.y - satellite.r) * k)
    }
    .frame(width: size, height: size, alignment: .topLeading)
  }
}

struct WidgetView: View {
  var entry: WorkflowEntry
  @Environment(\.widgetFamily) private var family

  /// The dark ramp's page stop from `styles/sentinel-tokens.css`, mirrored here because a widget
  /// cannot read CSS vars. One near-black canvas, one off-white ink — the app's two colours.
  static let canvas = Color(red: 0.082, green: 0.082, blue: 0.082)
  static let ink = Color(red: 0.980, green: 0.980, blue: 0.980)

  var body: some View {
    if let w = entry.workflow {
      // The whole tile is the tap target. A widget button people have to aim at is one they open
      // the app instead of using.
      Link(destination: URL(string: "olharai://run/\(w.id)")!) {
        VStack(alignment: .leading, spacing: 8) {
          EyeMark(size: family == .systemSmall ? 30 : 34, bg: Self.canvas)
          Spacer(minLength: 0)
          Text(w.name)
            .font(.system(size: family == .systemSmall ? 15 : 17, weight: .semibold))
            .lineLimit(2)
            .minimumScaleFactor(0.75)
            .multilineTextAlignment(.leading)
          Text("Tap to run")
            .font(.system(size: 12))
            .opacity(0.6)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
      }
      .buttonStyle(.plain)
    } else {
      VStack(alignment: .leading, spacing: 8) {
        EyeMark(size: 30, bg: Self.canvas)
        Spacer(minLength: 0)
        Text(entry.appNeverOpened
             ? "Open the app once, then long-press here → Edit Widget."
             : "Long-press → Edit Widget to choose a workflow.")
          .font(.system(size: 12))
          .opacity(0.6)
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }
  }
}

@main
struct OlharAIWidget: Widget {
  var body: some WidgetConfiguration {
    AppIntentConfiguration(
      kind: "OlharAIWidget",
      intent: SelectWorkflowIntent.self,
      provider: Provider()
    ) { entry in
      // The ink theme, not the system's translucent fill: one near-black canvas and one off-white
      // ink, exactly the two the app uses. `#151515` is the dark ramp's page stop from
      // `styles/sentinel-tokens.css`, mirrored here because a widget cannot read CSS vars.
      WidgetView(entry: entry)
        .foregroundStyle(WidgetView.ink)
        .containerBackground(for: .widget) { WidgetView.canvas }
    }
    .configurationDisplayName("Run a workflow")
    .description("One tap to run a workflow you choose.")
    .supportedFamilies([.systemSmall, .systemMedium])
  }
}
