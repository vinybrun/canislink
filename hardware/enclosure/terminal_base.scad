// CanisLink terminal base — OpenSCAD
// Units: mm. Export STL for prototype print.
// Feature: Presence — mat + console footprint.

$fn = 64;

mat_w = 450;
mat_d = 350;
mat_h = 18;
wall = 3;
console_w = 280;
console_d = 80;
console_h = 160;
pad_d = 100;
pad_spacing = 110;

module presence_mat() {
    difference() {
        // outer shell
        hull() {
            translate([10,10,0]) cylinder(h=mat_h, r=10);
            translate([mat_w-10,10,0]) cylinder(h=mat_h, r=10);
            translate([10,mat_d-10,0]) cylinder(h=mat_h, r=10);
            translate([mat_w-10,mat_d-10,0]) cylinder(h=mat_h, r=10);
        }
        // load cell cavity
        translate([mat_w/2-40, mat_d/2-30, 2])
            cube([80, 60, mat_h]);
        // cable channel
        translate([mat_w/2-8, mat_d-40, 4])
            cube([16, 50, 8]);
    }
    // non-slip ribbing
    for (i = [0:6]) {
        translate([30 + i*55, 25, mat_h])
            cube([8, mat_d-50, 1.2]);
    }
}

module button_pad(label_n) {
    color([0.2, 0.4, 0.9])
    difference() {
        cylinder(h=18, d=pad_d);
        translate([0,0,14]) cylinder(h=6, d=pad_d-12);
    }
    // stem
    translate([0,0,-8]) cylinder(h=8, d=20);
}

module console() {
    translate([(mat_w-console_w)/2, mat_d + 10, 0]) {
        difference() {
            cube([console_w, console_d, console_h]);
            translate([wall, wall, wall])
                cube([console_w-2*wall, console_d-2*wall, console_h]);
            // display cutout
            translate([20, console_d-wall-1, 40])
                cube([console_w-40, wall+2, 70]);
            // camera hole
            translate([console_w/2, console_d-wall-1, 130])
                rotate([-90,0,0]) cylinder(h=wall+2, d=12);
        }
        // four pads on top shelf
        for (i = [0:3]) {
            translate([40 + i*pad_spacing, console_d/2, console_h])
                button_pad(i);
        }
    }
}

module assembly() {
    color([0.15,0.15,0.15]) presence_mat();
    color([0.85,0.85,0.9]) console();
}

assembly();
