// The home-screen widget (card #178): the workflows you starred, one tap each.
//
// A widget extension is a separate process with its own sandbox. It holds NO credentials and speaks
// to no API — every button is a `Link` to `olharai://run/<id>`, which the app already handles. That
// is deliberate: a widget that called the control plane itself would be a second implementation of
// "run a workflow", with its own auth, its own error handling and its own bugs. It opens the app,
// and the app does the thing it already knows how to do.
//
// The data comes through the shared App Group container, written by the app whenever the starred
// set changes. If nothing has been starred yet the widget says so, rather than rendering an empty
// tile that reads as broken.
import SwiftUI
import WidgetKit

/// One starred workflow, as the app writes it into the App Group.
struct PinnedWorkflow: Codable, Identifiable, Hashable {
  let id: String
  let name: String
}

struct WorkflowEntry: TimelineEntry {
  let date: Date
  let workflows: [PinnedWorkflow]
}

/// Reads the starred list the app shares. Never network, never disk beyond the container.
enum SharedPins {
  static let suite = "group.com.olharai.app"
  static let key = "pinned_workflows"

  static func read() -> [PinnedWorkflow] {
    guard
      let defaults = UserDefaults(suiteName: suite),
      let raw = defaults.string(forKey: key),
      let data = raw.data(using: .utf8),
      let decoded = try? JSONDecoder().decode([PinnedWorkflow].self, from: data)
    else { return [] }
    // Four is what fits a medium tile without the labels truncating into nonsense. Starring more
    // is allowed; the widget shows the first four and the app shows them all.
    return Array(decoded.prefix(4))
  }
}

struct Provider: TimelineProvider {
  func placeholder(in context: Context) -> WorkflowEntry {
    WorkflowEntry(
      date: Date(),
      workflows: [PinnedWorkflow(id: "", name: "Open the garage")]
    )
  }

  func getSnapshot(in context: Context, completion: @escaping (WorkflowEntry) -> Void) {
    completion(WorkflowEntry(date: Date(), workflows: SharedPins.read()))
  }

  func getTimeline(in context: Context, completion: @escaping (Timeline<WorkflowEntry>) -> Void) {
    // `.never` on purpose: this widget shows a list of buttons, not a countdown or a live value.
    // Nothing about it goes stale on a clock — it changes when the app writes a new set and calls
    // `reloadAllTimelines`, and asking iOS to refresh it on a schedule would spend the system's
    // widget budget to redraw the same four names.
    completion(Timeline(entries: [WorkflowEntry(date: Date(), workflows: SharedPins.read())], policy: .never))
  }
}

/// One button. Big enough to hit without looking, because that is the whole point of a widget.
struct RunButton: View {
  let workflow: PinnedWorkflow

  var body: some View {
    Link(destination: URL(string: "olharai://run/\(workflow.id)")!) {
      HStack(spacing: 8) {
        Text(workflow.name)
          .font(.system(size: 15, weight: .semibold))
          .lineLimit(1)
          .minimumScaleFactor(0.8)
        Spacer(minLength: 4)
        Image(systemName: "play.fill").font(.system(size: 11, weight: .bold))
      }
      .padding(.horizontal, 12)
      .padding(.vertical, 10)
      .frame(maxWidth: .infinity)
      .background(Color.primary.opacity(0.10))
      .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
  }
}

struct WidgetView: View {
  var entry: Provider.Entry

  var body: some View {
    if entry.workflows.isEmpty {
      VStack(alignment: .leading, spacing: 6) {
        Text("Olhar AI").font(.system(size: 13, weight: .semibold))
        Text("Star a workflow in the app to put it here.")
          .font(.system(size: 12))
          .foregroundStyle(.secondary)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    } else {
      VStack(spacing: 6) {
        ForEach(entry.workflows) { w in
          RunButton(workflow: w)
        }
      }
    }
  }
}

@main
struct OlharAIWidget: Widget {
  var body: some WidgetConfiguration {
    StaticConfiguration(kind: "OlharAIWidget", provider: Provider()) { entry in
      if #available(iOS 17.0, *) {
        WidgetView(entry: entry).containerBackground(.fill.tertiary, for: .widget)
      } else {
        WidgetView(entry: entry).padding()
      }
    }
    .configurationDisplayName("Run a workflow")
    .description("One tap to run the workflows you starred.")
    .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
  }
}
