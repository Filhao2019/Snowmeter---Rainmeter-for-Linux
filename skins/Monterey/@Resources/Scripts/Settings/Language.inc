[Variables]
WidgetHeight=350
@include1=#@#Languages\all.inc
@Include2=#@#Scripts\Contexts\Languages.inc
@Include3=#@#Scripts\Contexts\Units.inc

[Language]
Meter=Shape
Shape=Rectangle 20,20,(#WidgetWidth# - #SidebarWidth# - 40),80,6 | Fill Color #ForegroundColor#05 | StrokeWidth 0
X=#SidebarWidth#
Y=55
DynamicVariables=1

[LanguageLabel]
Meter=String
Text=#tLanguage#
FontFace=#FontFace#
FontWeight=400
FontColor=#ForegroundColor#
FontSize=11
AntiAlias=1
X=(#SidebarWidth#+30)
Y=(85)
DynamicVariables=1

[LanguageValue]
Meter=String
MeterStyle=LanguageLabel
Text=[#[#Language]]    ▼
FontWeight=100
Padding=2,2,2,2
StringAlign=Right
Y=(85 - 2)
X=(#WidgetWidth# - 30)
W=76
LeftMouseUpAction=[!SetVariable Context "Languages"][!UpdateMeasure SetLanguageTickMark][!SkinCustomMenu]

[Line1]
Meter=Shape
Shape=Rectangle (#SidebarWidth# + 20),(115),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080

[TemperatureLabel]
Meter=String
MeterStyle=LanguageLabel
Text=#tTemperature#
Y=125

[TemperatureValue]
Meter=String
MeterStyle=LanguageLabel
Padding=2,2,2,2
FontWeight=100
StringAlign=Right
Y=(125 - 2)
X=(#WidgetWidth# - 30)
W=76
Text=[#t[#Units]]    ▼
LeftMouseUpAction=[!SetVariable Context "Units"][!UpdateMeasure SetUnitsTickMark][!SkinCustomMenu]
