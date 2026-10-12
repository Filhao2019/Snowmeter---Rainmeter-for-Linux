[Rainmeter]
DefaultUpdateDivider=-1
RightMouseUpAction=[]
Group=Monterey

[BackgroundMeter]
Meter = Shape
DynamicVariables = 1
Shape = Rectangle 0, 0, #WidgetWidth#, #WidgetHeight#, #WidgetRadius#, #WidgetRadius# | Fill Color #BackgroundColor# | StrokeWidth 0
Padding = #WidgetPadding#, #WidgetPadding#, #WidgetPadding#, #WidgetPadding#

[Variables]
Context=Empty
WidgetRadius = #RadiusBase#
@Include1=#@#Variables\Global.inc
@Include2=#@#Languages\#Language#\Widgets\#WidgetName#.inc
@Include3=#@#Languages\#Language#\Widget.inc
@Include4=#@#Scripts\Themes\#Theme#.inc
@Include5=#@#Scripts\Sizes\Large.inc
@Include6=#@#Variables\Layout.inc

WidgetCenterX = (#WidgetPadding# + #WidgetWidth# / 2)
WidgetCenterY = (#WidgetPadding# + #WidgetHeight# / 2)

[WhatsNew]
Meter=String
Text=#tWhatsNew# #WidgetName#
FontSize=(#WidgetWidth# * 0.05)
FontFace=#FontFace#
FontColor=#ForegroundColor#
DynamicVariables=1
FontWeight=600
AntiAlias=1
StringAlign=Center
X=#WidgetCenterX#
Y=(#WidgetPadding# + #WidgetHeight# * 0.075)
ClipString=1

[Icon1]
Meter=String
Text=#Icon1#
FontSize=(#WidgetWidth# * 0.1)
FontFace=#FontFace#
FontColor=#ForegroundColor#40
DynamicVariables=1
StringAlign=Center
AntiAlias=1
X=(#WidgetPadding# + #WidgetWidth# * 0.13)
Y=(#WidgetPadding# + #WidgetHeight# * 0.30)

[Title1]
Meter=String
Text=#Title1#
FontSize=(#WidgetWidth# * 0.04)
FontFace=#FontFace#
FontColor=#ForegroundColor#
FontWeight=600
DynamicVariables=1
AntiAlias=1
ClipString=1
X=(#WidgetPadding# + #WidgetWidth# * 0.25)
Y=(#WidgetPadding# + #WidgetHeight# * 0.30)
W=(#WidgetWidth# * 0.65)

[Text1]
Meter=String
Text=#Text1#
FontSize=(#WidgetWidth# * 0.035)
FontFace=#FontFace#
FontColor=#ForegroundColor#
DynamicVariables=1
AntiAlias=1
X=(#WidgetPadding# + #WidgetWidth# * 0.25)
Y=(#WidgetPadding# + #WidgetHeight# * 0.37)
W=(#WidgetWidth# * 0.68)
H=(#WidgetHeight# * 0.12)
ClipString=1

[Icon2]
Meter=String
MeterStyle=Icon1
Text=#Icon2#
Y=(#WidgetPadding# + #WidgetHeight# * 0.55)

[Title2]
Meter=String
MeterStyle=Title1
Text=#Title2#
Y=(#WidgetPadding# + #WidgetHeight# * 0.55)

[Text2]
Meter=String
MeterStyle=Text1
Text=#Text2#
Y=(#WidgetPadding# + #WidgetHeight# * 0.62)

[Line]
Meter=Shape
Shape=Rectangle 0,0,#WidgetWidth#,1 | Fill Color 80808080 | StrokeWidth 0
X=#WidgetPadding#
Y=(#WidgetPadding#+#WidgetHeight#*0.86)

[Close]
Meter=String
Text=#tContinue#
AntiAlias=1
FontSize=(#WidgetWidth# * 0.04)
FontWeight=600
FontFace=#FontFace#
FontColor=#ForegroundColor#
StringAlign=Center
DynamicVariables=1
X=#WidgetCenterX#
Y=(#WidgetPadding#+#WidgetHeight#*0.9)
W=(#WidgetWidth#/2)
LeftMouseUpAction=[!WriteKeyValue Variables "[#WidgetName]WidgetSize" "Medium" "#@#Variables\Layout.inc"][!ActivateConfig "#CURRENTCONFIG#" "Medium.ini"]
Hidden=0