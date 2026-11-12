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

    @State private var prompt: Prompt?
    @State private var promptShown = false
    @State private var name = ""
    @State private var confirmDelete = false

    private var label: String { (vm.currentName ?? "Osparad profil") + (vm.isDirty ? " •" : "") }
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
            Button("Spara") {
                if !vm.save() { startSaveAs() }
            }
            .keyboardShortcut("s", modifiers: .command)
            .disabled(hasSource && !vm.isDirty)
            Button("Spara som…") { startSaveAs() }
            Button("Byt namn…") {
                guard let id = vm.sourceID else { return }
                name = vm.currentName ?? ""
                prompt = .rename(id); promptShown = true
            }
            .disabled(!hasSource)
            Button("Duplicera") {
                if let id = vm.sourceID { vm.duplicate(id) }
            }
            .disabled(!hasSource)
            Button("Ta bort…") { confirmDelete = true }
                .disabled(!hasSource)
        } label: {
            Text(label).lineLimit(1)
        }
        .menuStyle(.button)
        .fixedSize()
        .frame(maxWidth: 220)
        .accessibilityLabel("Profil")
        .alert(promptTitle, isPresented: $promptShown, presenting: prompt) { p in
            TextField("Namn", text: $name)
            Button(promptButton(p)) { submit(p) }
            Button("Avbryt", role: .cancel) {}
        }
        .confirmationDialog("Ta bort ”\(vm.currentName ?? "")”?", isPresented: $confirmDelete) {
            Button("Ta bort", role: .destructive) {
                if let id = vm.sourceID { vm.delete(id) }
            }
            Button("Avbryt", role: .cancel) {}
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            vm.refreshSaved()
        }
    }

    private var promptTitle: String {
        if case .rename = prompt { return "Byt namn" }
        return "Spara som"
    }

    private func promptButton(_ p: Prompt) -> String {
        if case .rename = p { return "Byt namn" }
        return "Spara"
    }

    private func startSaveAs() {
        // Suggest a fresh name: re-using the current one would be rejected as taken.
        name = vm.currentName.map { "\($0) ny" } ?? "Min profil"
        prompt = .saveAs; promptShown = true
    }

    private func submit(_ p: Prompt) {
        switch p {
        case .saveAs: vm.saveAs(name)
        case .rename(let id): vm.rename(id, to: name)
        }
    }
}
