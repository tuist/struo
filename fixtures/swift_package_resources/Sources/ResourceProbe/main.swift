import ImplicitAssets
import DeclaredAssets
import ImplicitStrings
import ObjCResources

checkImplicitAssets()
checkImplicitStrings()
precondition(ObjCResourceMessage() == "objc resource loaded")
try checkDeclaredAssets()
print("package resources loaded")
