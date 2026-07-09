// Single dog-facing pad (Dogosophy-inspired): large, blue, textured, convex.
// 100 mm diameter. Print TPU/silicone mold positive.

$fn = 80;
d = 100;
h = 16;

difference() {
    union() {
        // convex top
        scale([1,1,0.28]) sphere(d=d);
        cylinder(h=h*0.55, d=d);
    }
    // underside cavity for switch + LED
    translate([0,0,-1]) cylinder(h=h*0.45, d=d*0.72);
    // center stem hole
    translate([0,0,-1]) cylinder(h=h, d=18);
}

// texture bumps for paw grip
for (a = [0:30:330]) {
    for (r = [20, 30, 38]) {
        rotate([0,0,a]) translate([r,0,h*0.5])
            sphere(d=3.5);
    }
}
