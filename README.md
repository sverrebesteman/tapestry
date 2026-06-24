# tapestry

tapestry crochet chart tool. give it image and your colours, get a chart back.

## Install

Requires Rust.

```git clone https://github.com/sverrebesteman/tapestry```
```cd tapestry```
```cargo build --release```
```sudo ln -sf $(realpath target/release/tapestry) /usr/local/bin/tapestry```

## Usage

### image to chart
```tapestry chart image.png -p "#F5F5DC" "#2C2C2C" -w 60```

### generate a pattern
```tapestry generate -P checkerboard -p "#F5F5DC" "#2C2C2C" -w 40 --height 20```

### preview colours
```tapestry preview "#F5F5DC" "#2C2C2C"```

### estimate yarn
```tapestry estimate 60x40 -s sc -w worsted```

Add ```-g``` for grid, ```-r``` for ruler, ```-i``` for row instructions, ```-d``` for dithering, ```--track``` to track rows as you go.
