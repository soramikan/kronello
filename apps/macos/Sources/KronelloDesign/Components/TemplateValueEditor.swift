import SwiftUI


/// Closed typed public values. DataTable editing preserves the edition's columns.
public struct KRTemplateValueEditor: View {
    @Environment(\.krPalette) var p
    let value: [String:Any]
    let schema: [String:Any]
    let assets: [[String:Any]]
    let commit: ([String:Any]) -> Void
    public init(value: [String:Any], schema: [String:Any], assets: [[String:Any]], commit: @escaping ([String:Any]) -> Void) {
        self.value = value; self.schema = schema; self.assets = assets; self.commit = commit
    }
    var kind: String { value.krString("kind") }
    public var body: some View {
        Group {
            switch kind {
            case "string":
                KRTextField("",value:.constant(value.krString("value")),onCommit:{ commit(["kind":kind,"value":$0]) })
            case "enum":
                KRPopupButton("値",options:(schema["choices"] as? [String] ?? []).map { .init($0,$0) },selection:.constant(value.krString("value")),onSelect:{ commit(["kind":kind,"value":$0]) })
            case "bool":
                KRCheckbox("有効",isOn:Binding(get:{value["value"] as? Bool ?? false},set:{commit(["kind":kind,"value":$0])}))
            case "scalar", "angle", "length":
                KRNumberField(value:.constant(value.krNumber("value")),step:0.1,range:limits,accessibilityLabel:"公開入力の値",onCommit:{ _,next in commit(["kind":kind,"value":next]) })
            case "vec2", "vec3":
                HStack(spacing:KRSpace.space1) {
                    ForEach(Array((value["value"] as? [Double] ?? []).enumerated()),id:\.offset) { index,number in
                        KRNumberField(value:.constant(number),step:0.1,accessibilityLabel:"成分 \(index)",onCommit:{ _,next in var numbers = value["value"] as? [Double] ?? []; numbers[index] = next; commit(["kind":kind,"value":numbers]) })
                    }
                }
            case "color": color
            case "asset_ref":
                VStack(alignment:.leading,spacing:KRSpace.space2) {
                    Text("MediaSlot · 既知の素材を選択").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    KRPopupButton("素材",options:assets.map { .init($0.krString("id"),URL(fileURLWithPath:$0.krString("path")).lastPathComponent.isEmpty ? String($0.krString("id").prefix(8)):URL(fileURLWithPath:$0.krString("path")).lastPathComponent) },selection:.constant(value.krString("value")),onSelect:{commit(["kind":kind,"value":$0])})
                }.padding(KRSpace.space2).overlay { Rectangle().stroke(p.lineStrong,style:StrokeStyle(lineWidth:1,dash:[4,3])) }
            case "data_table": table
            default: KRErrorLine(.init("UNSUPPORTED_FEATURE","この公開入力の型は編集できません"))
            }
        }
    }
    var limits: ClosedRange<Double>? {
        guard let minimum = schema["minimum"] as? Double, let maximum = schema["maximum"] as? Double, minimum <= maximum else { return nil }
        return minimum...maximum
    }
    var color: some View {
        let raw = value.krObject("value"), c = raw.krObject("components")
        return VStack(alignment:.leading,spacing:KRSpace.space1) {
            HStack(spacing:KRSpace.space2) {
                Rectangle().fill(Color(.sRGB,red:c.krNumber("r"),green:c.krNumber("g"),blue:c.krNumber("b"),opacity:c.krNumber("alpha"))).frame(width:16,height:16).overlay { Rectangle().strokeBorder(p.lineStrong,lineWidth:1) }
                Text(raw.krString("space")).krText(KRType.ruler).foregroundStyle(p.inkMuted)
            }
            ForEach(["r","g","b","alpha"],id:\.self) { channel in
                KRInspectorSettingRow(channel) {
                    KRNumberField(value:.constant(c.krNumber(channel)),step:0.01,range:0...1,precision:2,accessibilityLabel:channel,onCommit:{ _,next in var changed = c; changed[channel] = next; var updated = raw; updated["components"] = changed; commit(["kind":"color","value":updated]) }).frame(width:88)
                }
            }
        }
    }
    var table: some View {
        let raw = value.krObject("value"), columns = raw.krObject("columns").keys.sorted(), rows = raw.krObjects("rows")
        return ScrollView(.horizontal) {
            VStack(alignment:.leading,spacing:KRSpace.space2) {
                Text("DataTable · \(rows.count) 行 · \(columns.count) 列").krText(KRType.caption).foregroundStyle(p.inkMuted)
                ForEach(Array(rows.enumerated()),id:\.offset) { rowIndex,row in
                    HStack(alignment:.top,spacing:KRSpace.space2) {
                        Text(String(rowIndex + 1)).krText(KRType.ruler).foregroundStyle(p.inkMuted)
                        ForEach(columns,id:\.self) { column in
                            VStack(alignment:.leading,spacing:KRSpace.space1) {
                                Text(column).krText(KRType.ruler)
                                // DataTable's accepted cell kinds are not nested tables.
                                cell(row.krObject(column)) { next in var changedRows = rows; changedRows[rowIndex][column] = next; var changed = raw; changed["rows"] = changedRows; commit(["kind":"data_table","value":changed]) }
                            }.frame(width:180)
                        }
                    }
                }
            }
        }
    }
    @ViewBuilder func cell(_ cell: [String:Any], commit: @escaping ([String:Any]) -> Void) -> some View {
        switch cell.krString("kind") {
        case "string": KRTextField("",value:.constant(cell.krString("value")),onCommit:{commit(["kind":"string","value":$0])})
        case "scalar": KRNumberField(value:.constant(cell.krNumber("value")),step:0.1,accessibilityLabel:"セル",onCommit:{_,next in commit(["kind":"scalar","value":next])})
        case "bool": KRCheckbox("有効",isOn:Binding(get:{cell["value"] as? Bool ?? false},set:{commit(["kind":"bool","value":$0])}))
        case "color":
            KRTextField("sRGB · #RRGGBB",value:.constant(colorHex(cell)),onCommit:{text in
                guard text.count == 7,text.first == "#",let rgb = UInt32(text.dropFirst(),radix:16) else { return }
                commit(["kind":"color","value":["space":"srgb","components":["r":Double((rgb >> 16)&255)/255,"g":Double((rgb >> 8)&255)/255,"b":Double(rgb&255)/255,"alpha":cell.krObject("value").krObject("components").krNumber("alpha")]]])
            })
        default: KRErrorLine(.init("INVALID_DATA_TABLE","セルの型を確認してください"))
        }
    }
    func colorHex(_ cell: [String:Any]) -> String {
        let c = cell.krObject("value").krObject("components")
        return String(format:"#%02X%02X%02X",Int(min(1,max(0,c.krNumber("r")))*255),Int(min(1,max(0,c.krNumber("g")))*255),Int(min(1,max(0,c.krNumber("b")))*255))
    }
}

private extension Dictionary where Key == String, Value == Any {
    func krObject(_ key: String) -> [String:Any] { self[key] as? [String:Any] ?? [:] }
    func krObjects(_ key: String) -> [[String:Any]] { self[key] as? [[String:Any]] ?? [] }
    func krString(_ key: String) -> String { self[key] as? String ?? "" }
    func krNumber(_ key: String) -> Double { (self[key] as? NSNumber)?.doubleValue ?? 0 }
}
