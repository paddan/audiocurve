import SwiftUI
import AudioCurveCore

/// Header menu for saved profile versions: load, save, save as, rename, duplicate, delete.
struct ProfileMenu: View {
    let vm: PluginViewModel

    private enum Prompt: Identifiable {
        case saveAs, rename(UUID)
        var id: String {
            switch self {
            case .saveAs: "saveAs"
            case .rename(let id): "rename-\(id)"
            }
        }
    }

    @ViewState private var prompt: Prompt?
    @ViewState private var promptShown = false
    @ViewState private var name = ""
    @ViewState private var confirmDelete = false

    private var label: String { (vm.currentName ?? String(localized: "Unsaved profile")) + (vm.isDirty ? " •" : "") }
    private var hasSource: Bool { vm.currentName != nil }

    var body: some View {
        Menu {
            ForEach(vm.savedProfiles) { saved in
                Button {
                    vm.load(saved.id)
                } label: {
                    if saved.id == vm.sourceID {
                        Label(saved.name, systemImage: "checkmark")
                    } else {
                        Text(saved.name)
                    }
                }
            }
            Divider()
            Button("Save") {
                if !vm.save() { startSaveAs() }
            }
            .keyboardShortcut("s", modifiers: .command)
            .disabled(hasSource && !vm.isDirty)
            Button("Save As…") { startSaveAs() }
            Button("Rename…") {
                guard let id = vm.sourceID else { return }
                name = vm.currentName ?? ""
                prompt = .rename(id); promptShown = true
            }
            .disabled(!hasSource)
            Button("Duplicate") {
                if let id = vm.sourceID { vm.duplicate(id) }
            }
            .disabled(!hasSource)
            Button("Delete…") { confirmDelete = true }
                .disabled(!hasSource)
        } label: {
            Text(label).lineLimit(1)
        }
        .menuStyle(.button)
        .fixedSize()
        .frame(maxWidth: 220)
        .accessibilityLabel(Text("Profile"))
        .alert(promptTitle, isPresented: $promptShown, presenting: prompt) { p in
            TextField("Name", text: $name)
            Button(promptButton(p)) { submit(p) }
            Button("Cancel", role: .cancel) {}
        }
        .confirmationDialog("Delete “\(vm.currentName ?? "")”?", isPresented: $confirmDelete) {
            Button("Delete", role: .destructive) {
                if let id = vm.sourceID { vm.delete(id) }
            }
            Button("Cancel", role: .cancel) {}
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            vm.refreshSaved()
        }
    }

    private var promptTitle: String {
        if case .rename = prompt { return String(localized: "Rename") }
        return String(localized: "Save As")
    }

    private func promptButton(_ p: Prompt) -> String {
        if case .rename = p { return String(localized: "Rename") }
        return String(localized: "Save")
    }

    private func startSaveAs() {
        // Suggest a fresh name: re-using the current one would be rejected as taken.
        name = vm.currentName.map { String(localized: "\($0) new", comment: "Suggested name for Save As: <current name> new") }
            ?? String(localized: "My profile", comment: "Default name for the first saved profile")
        prompt = .saveAs; promptShown = true
    }

    private func submit(_ p: Prompt) {
        switch p {
        case .saveAs: vm.saveAs(name)
        case .rename(let id): vm.rename(id, to: name)
        }
    }
}
