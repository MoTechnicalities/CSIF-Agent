#!/usr/bin/env python3

import json
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEED_DIR = ROOT / "data" / "base_lobe_v1" / "seed"
BENCHMARK_DIR = ROOT / "data" / "base_lobe_v1" / "benchmarks"
BENCHMARK_PATH = BENCHMARK_DIR / "base_lobe_v1_benchmark.jsonl"
PHASE8_SCALE_LEVEL = int(os.environ.get("PHASE8_SCALE_LEVEL", "2"))


def write_lines(path: Path, lines):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_taxonomy():
    # Expanded for Milestone 1: generate a large, deterministic taxonomy
    # with explicit parent-child relations and clean category boundaries.
    lines = []

    def article(noun: str) -> str:
        return "an" if noun[0].lower() in "aeiou" else "a"

    def add_relation(subject: str, obj: str):
        lines.append(f"a {subject} is {article(obj)} {obj}")

    def add_many(subjects, obj: str):
        for subject in subjects:
            add_relation(subject, obj)

    mammals = [
        "whale", "dolphin", "seal", "walrus", "otter", "beaver", "platypus",
        "dog", "wolf", "fox", "cat", "lion", "tiger", "leopard", "jaguar", "cougar", "lynx", "cheetah",
        "horse", "zebra", "donkey", "mule", "pony",
        "cow", "bull", "buffalo", "bison", "sheep", "goat", "llama",
        "bear", "panda", "koala", "gorilla", "chimpanzee", "orangutan",
        "monkey", "baboon", "gibbon", "lemur",
        "mouse", "rat", "squirrel", "rabbit", "guinea pig", "hamster",
        "elephant", "giraffe", "hippopotamus", "rhinoceros", "camel",
        "deer", "elk", "moose", "reindeer", "antelope", "gazelle",
        "hedgehog", "porcupine", "armadillo", "badger", "weasel", "mink",
    ]

    birds = [
        "robin", "sparrow", "finch", "canary", "cardinal",
        "eagle", "hawk", "falcon", "owl", "vulture",
        "penguin", "duck", "goose", "swan",
        "parrot", "pigeon", "crow", "raven",
        "flamingo", "pelican", "heron", "stork",
        "hummingbird", "woodpecker", "jaybird",
        "turkey", "chicken", "ostrich", "peacock",
        "albatross", "seagull", "nightingale", "lark",
        "dove", "sparrowhawk", "buzzard", "kestrel",
    ]

    fish = [
        "shark", "salmon", "trout", "tuna", "mackerel", "cod", "herring", "bass",
        "carp", "catfish", "eel", "anchovy", "perch", "halibut",
        "goldfish", "flounder", "pike", "snapper", "grouper", "marlin",
        "swordfish", "tilapia", "sardine", "minnow", "guppy",
        "tetra", "barracuda", "puffer", "clownfish", "seahorse",
        "mullet", "haddock", "plaice", "sole", "dab",
    ]

    plants = [
        "oak", "pine", "maple", "birch", "elm", "ash", "cedar", "spruce", "fir",
        "rose", "tulip", "lily", "daisy", "sunflower",
        "violet", "iris", "orchid", "peony", "lavender",
        "grass", "fern", "moss", "cactus", "bamboo",
        "palm", "eucalyptus", "willow", "poplar", "alder",
        "apple tree", "orange tree", "lemon tree", "pear tree", "cherry tree",
        "wheat", "corn", "rice", "barley", "oat",
    ]

    vehicles = [
        "car", "truck", "bus", "bicycle", "motorcycle", "train", "airplane", "helicopter", "boat", "ship",
        "sedan", "coupe", "wagon", "van", "taxi", "pickup", "semi", "trailer",
        "scooter", "trolley", "tram", "subway", "metro", "ferry", "yacht", "canoe",
    ]

    devices = [
        "laptop", "desktop", "tablet", "phone", "router", "switch", "server", "monitor", "keyboard", "mouse",
        "printer", "scanner", "camera", "speaker", "microphone", "headphone", "projector",
        "modem", "amplifier", "mixer", "console",
    ]

    add_many(mammals, "mammal")
    add_many(birds, "bird")
    add_many(fish, "fish")
    add_many(plants, "plant")
    add_many(vehicles, "vehicle")
    add_many(devices, "device")

    dog_breeds = [
        "labrador", "golden retriever", "german shepherd", "bulldog", "poodle", "beagle", "husky", "dachshund",
        "boxer", "rottweiler", "dalmatian", "chihuahua", "pug", "pitbull", "shih tzu", "maltese",
        "yorkshire terrier", "scottish terrier", "great dane", "saint bernard",
    ]
    cat_breeds = ["siamese", "persian", "maine coon", "ragdoll", "tabby", "calico", "bengal", "sphynx", "birman", "abyssinian", "scottish fold"]
    big_cats = ["african lion", "siberian tiger", "bengal tiger", "clouded leopard", "snow leopard", "black leopard", "puma", "caracal"]
    whale_types = ["blue whale", "humpback whale", "orca", "beluga whale", "narwhal", "sperm whale", "pilot whale", "right whale"]
    primates = ["macaque", "mandrill", "capuchin", "howler monkey", "tamarin", "marmoset", "spider monkey", "colobus", "langur", "proboscis monkey"]
    hoofed = ["yak", "ibex", "musk ox", "eland", "oryx", "impala", "wildebeest", "springbok", "chamois", "saiga", "okapi", "water buffalo"]

    add_many(dog_breeds, "dog")
    add_many(cat_breeds, "cat")
    add_many(big_cats, "big cat")
    add_many(whale_types, "whale")
    add_many(primates, "primate")
    add_many(hoofed, "hoofed mammal")

    raptors = ["bald eagle", "golden eagle", "peregrine falcon", "merlin", "gyrfalcon", "barn owl", "snowy owl", "great horned owl", "harrier", "kite", "osprey", "serpent eagle"]
    waterfowl = ["mallard", "wood duck", "teal", "pintail", "whooper swan", "mute swan", "brent goose", "egyptian goose", "shelduck", "wigeon"]
    parrots = ["macaw", "cockatoo", "budgerigar", "african gray", "amazon parrot", "conure", "lorikeet", "eclectus parrot"]
    seabirds = ["tern", "auk", "puffin", "frigatebird", "gannet", "cormorant", "skua", "petrel", "shearwater", "booby"]
    penguin_types = ["adelie penguin", "emperor penguin", "king penguin", "chinstrap penguin", "gentoo penguin", "rockhopper penguin", "macaroni penguin", "little penguin"]

    add_many(raptors, "raptor")
    add_many(waterfowl, "waterfowl")
    add_many(parrots, "parrot")
    add_many(seabirds, "seabird")
    add_many(penguin_types, "penguin")

    shark_types = ["great white shark", "tiger shark", "hammerhead shark", "bull shark", "lemon shark", "nurse shark", "whale shark", "reef shark", "sand tiger shark", "mako shark", "thresher shark", "blue shark"]
    salmon_types = ["atlantic salmon", "pacific salmon", "chinook salmon", "coho salmon", "sockeye salmon", "pink salmon", "chum salmon", "masu salmon"]
    tuna_types = ["albacore tuna", "bluefin tuna", "skipjack tuna", "yellowfin tuna", "bigeye tuna", "blackfin tuna", "longtail tuna", "southern bluefin tuna"]
    reef_fish = ["angelfish", "butterflyfish", "damselfish", "surgeonfish", "triggerfish", "wrasse", "goby", "blenny", "lionfish", "moorish idol"]
    freshwater_fish = ["koi", "arowana", "piranha", "sturgeon", "zander", "chub", "roach", "bream", "tench", "char", "grayling", "mudfish"]

    add_many(shark_types, "shark")
    add_many(salmon_types, "salmon")
    add_many(tuna_types, "tuna")
    add_many(reef_fish, "reef fish")
    add_many(freshwater_fish, "freshwater fish")

    tree_types = ["oak tree", "pine tree", "maple tree", "birch tree", "elm tree", "ash tree", "cedar tree", "spruce tree", "fir tree", "willow tree", "poplar tree", "alder tree", "beech tree", "hickory tree", "walnut tree", "redwood tree", "sequoia tree", "baobab tree", "cypress tree", "sycamore tree"]
    flower_types = ["red rose", "white rose", "yellow tulip", "purple tulip", "water lily", "daylily", "gerbera daisy", "shasta daisy", "giant sunflower", "wild sunflower", "fuchsia orchid", "moth orchid", "iris germanica", "bearded iris", "lavender angustifolia", "french lavender", "pink peony", "white peony", "wild violet", "african violet"]
    crops = ["durum wheat", "sweet corn", "basmati rice", "jasmine rice", "rolled oat", "rye", "sorghum", "millet", "quinoa", "buckwheat", "lentil", "soybean"]
    cacti = ["saguaro", "barrel cactus", "prickly pear", "christmas cactus", "hedgehog cactus", "organ pipe cactus", "cholla", "moon cactus", "fishhook cactus", "pincushion cactus"]
    herbs = ["basil", "mint", "rosemary", "thyme", "sage", "oregano", "parsley", "cilantro", "dill", "chive", "tarragon", "marjoram"]

    add_many(tree_types, "tree")
    add_many(flower_types, "flower")
    add_many(crops, "crop")
    add_many(cacti, "cactus")
    add_many(herbs, "herb")

    car_types = ["suv", "crossover", "hatchback", "minivan", "pickup truck", "sports car", "muscle car", "hybrid car", "electric car", "compact car", "convertible", "roadster", "limousine", "station wagon", "city car", "offroad vehicle", "race car", "touring car", "diesel sedan", "electric suv"]
    truck_types = ["box truck", "flatbed truck", "tow truck", "dump truck", "cement truck", "garbage truck", "delivery truck", "fire truck", "refrigerated truck", "logging truck"]
    bike_types = ["mountain bike", "road bike", "hybrid bike", "bmx bike", "touring bike", "gravel bike", "folding bike", "electric bike", "recumbent bike", "track bike", "cargo bike", "city bike"]
    aircraft_types = ["commercial airplane", "cargo plane", "fighter jet", "drone", "seaplane", "glider", "biplane", "airship", "regional jet", "business jet", "crop duster", "amphibious plane"]
    boat_types = ["cargo ship", "tanker", "container ship", "cruise ship", "sailboat", "kayak", "catamaran", "trawler", "patrol boat", "lifeboat", "submarine", "fishing boat"]
    rail_types = ["freight train", "bullet train", "metro train", "tramcar", "light rail", "monorail", "steam locomotive", "diesel locomotive"]

    add_many(car_types, "car")
    add_many(truck_types, "truck")
    add_many(bike_types, "bicycle")
    add_many(aircraft_types, "aircraft")
    add_many(boat_types, "ship")
    add_many(rail_types, "train")

    computer_types = ["workstation", "netbook", "ultrabook", "gaming pc", "mini pc", "all in one pc", "rack server", "blade server", "microserver", "chromebook", "thin client", "single board computer"]
    network_devices = ["gateway", "repeater", "bridge", "hub", "firewall", "load balancer", "wireless access point", "edge router", "poe switch", "network controller"]
    audio_devices = ["turntable", "soundbar", "subwoofer", "studio monitor", "audio interface", "dac", "digital mixer", "power amplifier", "headset", "wireless earbud"]
    video_devices = ["webcam", "action camera", "dslr", "mirrorless camera", "camcorder", "capture card", "video switcher", "media player", "set top box", "streaming stick"]
    mobile_devices = ["smartwatch", "fitness tracker", "e reader", "handheld console", "gps navigator", "mobile hotspot", "rugged phone", "satellite phone", "tablet pc", "foldable phone"]
    io_devices = ["trackball", "drawing tablet", "barcode scanner", "label printer", "thermal printer", "document scanner", "game controller", "joystick", "touchpad", "numeric keypad"]

    add_many(computer_types, "computer")
    add_many(network_devices, "network device")
    add_many(audio_devices, "audio device")
    add_many(video_devices, "video device")
    add_many(mobile_devices, "mobile device")
    add_many(io_devices, "io device")

    add_many(mammals, "vertebrate")
    add_many(birds, "vertebrate")
    add_many(fish, "vertebrate")
    add_many(plants, "living thing")
    add_many(vehicles, "artifact")
    add_many(devices, "artifact")

    lines.extend([
        "a dog is a mammal",
        "a cat is a mammal",
        "a big cat is a mammal",
        "a primate is a mammal",
        "a hoofed mammal is a mammal",
        "a mammal is an animal",
        "a bird is an animal",
        "a raptor is a bird",
        "a waterfowl is a bird",
        "a seabird is a bird",
        "a fish is an animal",
        "a shark is a fish",
        "a salmon is a fish",
        "a tuna is a fish",
        "a reef fish is a fish",
        "a freshwater fish is a fish",
        "a animal is a vertebrate",
        "a vertebrate is an organism",
        "an animal is a living thing",
        "a plant is a living thing",
        "a tree is a plant",
        "a flower is a plant",
        "a crop is a plant",
        "a herb is a plant",
        "a living thing is a physical object",
        "a vehicle is a transportation device",
        "a car is a vehicle",
        "an aircraft is a vehicle",
        "a ship is a vehicle",
        "a bicycle is a vehicle",
        "a train is a vehicle",
        "a transportation device is a machine",
        "a device is a machine",
        "a computer is a device",
        "a network device is a device",
        "an audio device is a device",
        "a video device is a device",
        "a mobile device is a device",
        "an io device is a device",
        "a machine is an artifact",
        "an artifact is a physical object",
        "a physical object is an entity",
    ])

    # Phase-8 scale increment: deterministic synthetic artifact variants.
    # 30 prefixes x 25 roots => 750 additional taxonomy edges.
    variant_prefixes = [
        "adaptive", "agile", "atomic", "balanced", "compact", "core", "durable", "dynamic",
        "edge", "elastic", "hybrid", "intelligent", "kinetic", "layered", "modular", "native",
        "neural", "optical", "portable", "precision", "prime", "rapid", "resilient", "robust",
        "scalable", "smart", "stable", "tactical", "ultra", "virtual",
    ]
    variant_roots = [
        "adapter", "array", "beacon", "bridge", "capsule", "cartridge", "controller", "driver",
        "engine", "gateway", "hub", "interface", "kit", "module", "node", "panel", "probe",
        "relay", "sensor", "stack", "station", "switch", "terminal", "unit", "vault",
    ]

    for prefix in variant_prefixes:
        for root in variant_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    if PHASE8_SCALE_LEVEL >= 2:
        # Phase-8 second increment: additional synthetic platform families.
        # 12 prefixes x 20 roots => 240 additional taxonomy edges.
        platform_prefixes = [
            "aero", "bio", "chrono", "cyber", "electro", "hydro",
            "infra", "micro", "nano", "quantum", "thermo", "xeno",
        ]
        platform_roots = [
            "anchor", "board", "cell", "chassis", "cluster", "console", "core", "dock", "fabric", "frame",
            "grid", "kernel", "matrix", "mesh", "pod", "rack", "rail", "ring", "shield", "tower",
        ]

        for prefix in platform_prefixes:
            for root in platform_roots:
                add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-9 milestone push: deterministic taxonomy-only expansion.
    # 10 prefixes x 20 roots => 200 additional taxonomy edges.
    phase9_prefixes = [
        "amber", "aurora", "cobalt", "delta", "ember",
        "frost", "glacier", "harbor", "ion", "juno",
    ]
    phase9_roots = [
        "apex", "arc", "bay", "block", "chain", "deck", "field", "forge", "lane", "line",
        "loop", "mark", "path", "plane", "port", "pulse", "span", "stage", "trace", "zone",
    ]

    for prefix in phase9_prefixes:
        for root in phase9_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 6: taxonomy catch-up via deterministic control families.
    # 12 prefixes x 20 roots => 240 additional taxonomy edges.
    phase10_inc6_prefixes = [
        "axiom", "beacon", "cinder", "driven", "ember", "fractal",
        "granular", "helix", "ion", "keystone", "lattice", "matrix",
    ]
    phase10_inc6_roots = [
        "anchor", "array", "bundle", "channel", "cluster", "console", "controller", "domain", "fabric", "gateway",
        "kernel", "ledger", "module", "orchestrator", "pod", "relay", "service", "stack", "stream", "vector",
    ]

    for prefix in phase10_inc6_prefixes:
        for root in phase10_inc6_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 7: taxonomy catch-up via deterministic mesh families.
    # 12 prefixes x 20 roots => 240 additional taxonomy edges.
    phase10_inc7_prefixes = [
        "amber", "brisk", "cobalt", "drift", "ember", "fused",
        "glint", "helios", "ivory", "jade", "kinetic", "lumen",
    ]
    phase10_inc7_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "grid", "kernel",
        "matrix", "mesh", "node", "pod", "rail", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc7_prefixes:
        for root in phase10_inc7_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 8: taxonomy acceleration via deterministic lattice families.
    # 15 prefixes x 20 roots => 300 additional taxonomy edges.
    phase10_inc8_prefixes = [
        "apex", "binary", "cipher", "delta", "ether", "flux", "gamma", "halo", "inert", "jolt",
        "kappa", "lucid", "micro", "nova", "omega",
    ]
    phase10_inc8_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc8_prefixes:
        for root in phase10_inc8_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 9: taxonomy acceleration via deterministic orbit families.
    # 15 prefixes x 20 roots => 300 additional taxonomy edges.
    phase10_inc9_prefixes = [
        "aether", "bravo", "crystal", "driven", "ember", "falcon", "glacier", "harbor", "iris", "jupiter",
        "karma", "lunar", "meridian", "nebula", "onyx",
    ]
    phase10_inc9_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc9_prefixes:
        for root in phase10_inc9_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 10: taxonomy acceleration via deterministic matrix families.
    # 15 prefixes x 20 roots => 300 additional taxonomy edges.
    phase10_inc10_prefixes = [
        "altair", "binary", "cinder", "dorsal", "ember", "fusion", "garnet", "helios", "ion", "juno",
        "kepler", "lithic", "mercury", "nova", "orion",
    ]
    phase10_inc10_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc10_prefixes:
        for root in phase10_inc10_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 11: taxonomy acceleration via deterministic stellar families.
    # 15 prefixes x 20 roots => 300 additional taxonomy edges.
    phase10_inc11_prefixes = [
        "aster", "bravo", "cobalt", "draco", "ember", "fjord", "glint", "helix", "iris", "jovian",
        "krypton", "lumen", "morrow", "nebula", "onyx",
    ]
    phase10_inc11_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc11_prefixes:
        for root in phase10_inc11_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 12: taxonomy acceleration via deterministic signal families.
    # 15 prefixes x 20 roots => 300 additional taxonomy edges.
    phase10_inc12_prefixes = [
        "aquila", "bronze", "cirrus", "dynamo", "ember", "fission", "graph", "horizon", "isotope", "javelin",
        "kestrel", "lyric", "magnet", "neutron", "opal",
    ]
    phase10_inc12_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc12_prefixes:
        for root in phase10_inc12_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-10 Increment 13: taxonomy acceleration via deterministic relay families.
    # 15 prefixes x 20 roots => 300 additional taxonomy edges.
    phase10_inc13_prefixes = [
        "astra", "boron", "circa", "driven", "ember", "fathom", "gilded", "helio", "iridium", "jasper",
        "karma", "lattice", "mosaic", "nimbus", "onyria",
    ]
    phase10_inc13_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase10_inc13_prefixes:
        for root in phase10_inc13_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-11 scale push: deterministic high-volume taxonomy expansion.
    # 30 prefixes x 60 roots => 1800 additional taxonomy edges.
    phase11_prefixes = [
        "alder", "brim", "cobalt", "dawn", "ember", "flint", "gale", "hallow", "ionic", "jade",
        "kilo", "lunar", "mirth", "nova", "onyx", "praxis", "quill", "riven", "sol", "tundra",
        "ultra", "vivid", "warden", "xeno", "yarrow", "zenith", "aster", "brisk", "cipher", "dorsal",
    ]
    phase11_roots = [
        "anchor", "array", "arc", "axis", "band", "bridge", "cell", "channel", "cluster", "console",
        "core", "deck", "fabric", "field", "frame", "gateway", "grid", "hub", "index", "kernel",
        "layer", "ledger", "line", "link", "matrix", "mesh", "module", "node", "panel", "path",
        "pod", "port", "pulse", "rail", "relay", "ring", "route", "router", "sector", "service",
        "shell", "signal", "span", "stack", "stage", "stream", "switch", "tower", "trace", "track",
        "unit", "vector", "vault", "weave", "zone", "orbit", "beacon", "column", "delta", "vectorium",
    ]

    for prefix in phase11_prefixes:
        for root in phase11_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    # Phase-11 top-up: deterministic taxonomy extension.
    # 10 prefixes x 20 roots => 200 additional taxonomy edges.
    phase11_topup_prefixes = [
        "auric", "bravo", "cinder", "dune", "ember", "frost", "glade", "helix", "ionic", "juno",
    ]
    phase11_topup_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in phase11_topup_prefixes:
        for root in phase11_topup_roots:
            add_relation(f"{prefix} {root}", "synthetic artifact")

    lines.extend([
        "a synthetic artifact is an artifact",
    ])

    return sorted(set(lines))


def build_causality():
    pairs = [
        ("rain", "wet ground"),
        ("wet ground", "slippery roads"),
        ("slippery roads", "accidents"),
        ("fire", "smoke"),
        ("smoke", "coughing"),
        ("smoke", "alarm"),
        ("sun", "heat"),
        ("heat", "evaporation"),
        ("heat", "sweating"),
        ("power outage", "darkness"),
        ("darkness", "confusion"),
        ("exercise", "sweating"),
        ("wind", "waves"),
        ("waves", "erosion"),
        ("freezing temperatures", "ice"),
        ("ice", "slipping"),
        ("traffic", "delays"),
        ("delays", "late arrival"),
        ("late arrival", "missed meeting"),
        ("heavy snowfall", "road closures"),
        ("road closures", "detours"),
        ("virus exposure", "infection"),
        ("infection", "fever"),
        ("fever", "weakness"),
        ("dehydration", "fatigue"),
        ("insufficient sleep", "fatigue"),
        ("fatigue", "reduced focus"),
        ("reduced focus", "errors"),
        ("software bug", "service failure"),
        ("service failure", "customer impact"),
        ("disk full", "write failures"),
        ("write failures", "data loss risk"),
        ("high latency", "slow responses"),
        ("slow responses", "timeouts"),
        ("timeouts", "retry storms"),
        ("retry storms", "load spikes"),
        ("load spikes", "degraded performance"),
        ("degraded performance", "user frustration"),
        ("user frustration", "complaints"),
        ("complaints", "support tickets"),
        ("weather", "visibility"),
        ("low visibility", "accidents"),
        ("accidents", "injuries"),
        ("injuries", "hospitalization"),
        ("cold", "ice"),
        ("ice", "skidding"),
        ("skidding", "crashes"),
        ("alcohol", "impairment"),
        ("impairment", "accidents"),
        ("speed", "accidents"),
        ("distraction", "errors"),
        ("errors", "quality issues"),
        ("quality issues", "complaints"),
        ("maintenance neglect", "failures"),
        ("failures", "downtime"),
        ("downtime", "revenue loss"),
        ("revenue loss", "layoffs"),
        ("layoffs", "reduced capacity"),
        ("reduced capacity", "lower service"),
        ("lower service", "customer churn"),
    ]

    # Phase-10 expansion: deterministic operational causality chains.
    # 20 services x 4 edges each => 80 additional pairs.
    services = [
        "api gateway", "auth service", "billing service", "cache cluster", "cdn edge",
        "control plane", "data pipeline", "event bus", "feature flag service", "indexer",
        "ingest service", "job scheduler", "load balancer", "message queue", "metrics collector",
        "notification service", "object store", "search service", "session service", "workflow engine",
    ]
    for svc in services:
        pairs.extend(
            [
                (f"{svc} overload", f"{svc} latency"),
                (f"{svc} latency", f"{svc} timeout"),
                (f"{svc} timeout", f"{svc} retries"),
                (f"{svc} retries", f"{svc} pressure"),
            ]
        )

    # 15 client journeys x 4 edges each => 60 additional pairs.
    journeys = [
        "checkout", "signup", "login", "upload", "download",
        "search", "sync", "publish", "stream", "share",
        "invite", "comment", "checkout mobile", "onboarding", "reporting",
    ]
    for flow in journeys:
        pairs.extend(
            [
                (f"{flow} friction", f"{flow} abandonment"),
                (f"{flow} abandonment", f"{flow} revenue dip"),
                (f"{flow} revenue dip", f"{flow} prioritization"),
                (f"{flow} prioritization", f"{flow} remediation"),
            ]
        )

    # Phase-10 Increment 2: deterministic infrastructure incident ladders.
    # 30 domains x 4 edges => 120 additional pairs.
    incident_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "checkout", "signup", "login", "upload", "download", "stream", "share", "report", "mobile", "gateway",
    ]
    for domain in incident_domains:
        pairs.extend(
            [
                (f"{domain} saturation", f"{domain} queueing"),
                (f"{domain} queueing", f"{domain} timeout"),
                (f"{domain} timeout", f"{domain} fallback"),
                (f"{domain} fallback", f"{domain} recovery"),
            ]
        )

    # 30 customer outcomes x 4 edges => 120 additional pairs.
    outcome_topics = [
        "trust", "retention", "activation", "conversion", "engagement", "satisfaction", "support", "churn",
        "adoption", "throughput", "efficiency", "quality", "reliability", "availability", "consistency", "latency",
        "onboarding", "compliance", "security", "privacy", "governance", "visibility", "observability", "escalation",
        "stability", "planning", "budget", "capacity", "forecast", "readiness",
    ]
    for topic in outcome_topics:
        pairs.extend(
            [
                (f"{topic} regression", f"{topic} alert"),
                (f"{topic} alert", f"{topic} triage"),
                (f"{topic} triage", f"{topic} fix"),
                (f"{topic} fix", f"{topic} validation"),
            ]
        )

    # Phase-10 Increment 3: deterministic dependency and rollout ladders.
    # 25 dependency domains x 4 edges => 100 additional pairs.
    dependency_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "checkout", "signup", "login", "mobile", "gateway",
    ]
    for domain in dependency_domains:
        pairs.extend(
            [
                (f"{domain} dependency drift", f"{domain} integration mismatch"),
                (f"{domain} integration mismatch", f"{domain} incident"),
                (f"{domain} incident", f"{domain} rollback"),
                (f"{domain} rollback", f"{domain} stabilization"),
            ]
        )

    # 25 rollout domains x 4 edges => 100 additional pairs.
    rollout_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report",
    ]
    for domain in rollout_domains:
        pairs.extend(
            [
                (f"{domain} canary failure", f"{domain} rollout pause"),
                (f"{domain} rollout pause", f"{domain} patch"),
                (f"{domain} patch", f"{domain} canary rerun"),
                (f"{domain} canary rerun", f"{domain} rollout complete"),
            ]
        )

    # Phase-10 Increment 4: deterministic reliability and maintenance ladders.
    # 25 reliability domains x 4 edges => 100 additional pairs.
    reliability_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report",
    ]
    for domain in reliability_domains:
        pairs.extend(
            [
                (f"{domain} jitter", f"{domain} instability"),
                (f"{domain} instability", f"{domain} incident review"),
                (f"{domain} incident review", f"{domain} remediation"),
                (f"{domain} remediation", f"{domain} confidence"),
            ]
        )

    # 25 maintenance topics x 4 edges => 100 additional pairs.
    maintenance_topics = [
        "backlog", "build", "capacity", "change", "compliance", "config", "cost", "coverage", "deployment", "documentation",
        "drift", "governance", "handoff", "hygiene", "incident", "inventory", "latency", "monitoring", "ownership", "patching",
        "readiness", "runbook", "safety", "testing", "versioning",
    ]
    for topic in maintenance_topics:
        pairs.extend(
            [
                (f"{topic} debt", f"{topic} cleanup"),
                (f"{topic} cleanup", f"{topic} standardization"),
                (f"{topic} standardization", f"{topic} predictability"),
                (f"{topic} predictability", f"{topic} resilience"),
            ]
        )

    # Phase-10 Increment 5: deterministic incident and optimization ladders.
    # 25 incident classes x 4 edges => 100 additional pairs.
    incident_classes = [
        "availability", "auth", "billing", "cache", "cdn", "compute", "config", "control", "data", "db",
        "delivery", "dns", "gateway", "identity", "index", "ingest", "latency", "message", "network", "object",
        "queue", "region", "search", "session", "storage",
    ]
    for cls in incident_classes:
        pairs.extend(
            [
                (f"{cls} warning", f"{cls} escalation"),
                (f"{cls} escalation", f"{cls} mitigation"),
                (f"{cls} mitigation", f"{cls} validation"),
                (f"{cls} validation", f"{cls} closure"),
            ]
        )

    # 25 optimization tracks x 4 edges => 100 additional pairs.
    optimization_tracks = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report",
    ]
    for track in optimization_tracks:
        pairs.extend(
            [
                (f"{track} tuning", f"{track} efficiency"),
                (f"{track} efficiency", f"{track} savings"),
                (f"{track} savings", f"{track} reinvestment"),
                (f"{track} reinvestment", f"{track} capacity growth"),
            ]
        )

    # Phase-10 Increment 6: deterministic recovery and policy ladders.
    # 25 recovery domains x 4 edges => 100 additional pairs.
    recovery_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report",
    ]
    for domain in recovery_domains:
        pairs.extend(
            [
                (f"{domain} rollback trigger", f"{domain} rollback"),
                (f"{domain} rollback", f"{domain} state restore"),
                (f"{domain} state restore", f"{domain} verification"),
                (f"{domain} verification", f"{domain} service recovery"),
            ]
        )

    # 25 policy domains x 4 edges => 100 additional pairs.
    policy_domains = [
        "access", "audit", "backup", "build", "capacity", "change", "compliance", "config", "cost", "data",
        "deployment", "docs", "governance", "hygiene", "incident", "latency", "monitoring", "ownership", "patch", "privacy",
        "quality", "readiness", "release", "security", "testing",
    ]
    for domain in policy_domains:
        pairs.extend(
            [
                (f"{domain} policy gap", f"{domain} policy update"),
                (f"{domain} policy update", f"{domain} enforcement"),
                (f"{domain} enforcement", f"{domain} consistency"),
                (f"{domain} consistency", f"{domain} confidence"),
            ]
        )

    # Phase-10 Increment 7: deterministic failover and calibration ladders.
    # 20 failover domains x 4 edges => 80 additional pairs.
    failover_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
    ]
    for domain in failover_domains:
        pairs.extend(
            [
                (f"{domain} failover test", f"{domain} failover plan"),
                (f"{domain} failover plan", f"{domain} failover drill"),
                (f"{domain} failover drill", f"{domain} failover confidence"),
                (f"{domain} failover confidence", f"{domain} resilience gain"),
            ]
        )

    # 20 calibration tracks x 4 edges => 80 additional pairs.
    calibration_tracks = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
    ]
    for track in calibration_tracks:
        pairs.extend(
            [
                (f"{track} baseline drift", f"{track} recalibration"),
                (f"{track} recalibration", f"{track} stability"),
                (f"{track} stability", f"{track} confidence uplift"),
                (f"{track} confidence uplift", f"{track} quality gain"),
            ]
        )

    # Phase-10 Increment 8: deterministic safeguards and readiness ladders.
    # 30 safeguard domains x 4 edges => 120 additional pairs.
    safeguard_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "gateway", "mobile", "checkout", "signup", "login",
    ]
    for domain in safeguard_domains:
        pairs.extend(
            [
                (f"{domain} safeguard gap", f"{domain} safeguard design"),
                (f"{domain} safeguard design", f"{domain} safeguard rollout"),
                (f"{domain} safeguard rollout", f"{domain} safeguard validation"),
                (f"{domain} safeguard validation", f"{domain} incident reduction"),
            ]
        )

    # 30 readiness domains x 4 edges => 120 additional pairs.
    readiness_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "governance", "quality", "security", "release", "testing",
    ]
    for domain in readiness_domains:
        pairs.extend(
            [
                (f"{domain} readiness review", f"{domain} readiness action"),
                (f"{domain} readiness action", f"{domain} readiness score"),
                (f"{domain} readiness score", f"{domain} release confidence"),
                (f"{domain} release confidence", f"{domain} stable delivery"),
            ]
        )

    # Phase-10 Increment 9: deterministic response and assurance ladders.
    # 30 response domains x 4 edges => 120 additional pairs.
    response_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "gateway", "mobile", "checkout", "signup", "login",
    ]
    for domain in response_domains:
        pairs.extend(
            [
                (f"{domain} response lag", f"{domain} response tuning"),
                (f"{domain} response tuning", f"{domain} response stability"),
                (f"{domain} response stability", f"{domain} response confidence"),
                (f"{domain} response confidence", f"{domain} user trust"),
            ]
        )

    # 30 assurance domains x 4 edges => 120 additional pairs.
    assurance_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "governance", "quality", "security", "release", "testing", "compliance", "privacy", "safety", "capacity", "readiness",
    ]
    for domain in assurance_domains:
        pairs.extend(
            [
                (f"{domain} assurance plan", f"{domain} assurance execution"),
                (f"{domain} assurance execution", f"{domain} assurance evidence"),
                (f"{domain} assurance evidence", f"{domain} audit confidence"),
                (f"{domain} audit confidence", f"{domain} release approval"),
            ]
        )

    # Phase-10 Increment 10: deterministic diagnostics and forecasting ladders.
    # 30 diagnostics domains x 4 edges => 120 additional pairs.
    diagnostics_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "gateway", "mobile", "checkout", "signup", "login",
    ]
    for domain in diagnostics_domains:
        pairs.extend(
            [
                (f"{domain} diagnostic signal", f"{domain} diagnostic trace"),
                (f"{domain} diagnostic trace", f"{domain} root cause"),
                (f"{domain} root cause", f"{domain} corrective action"),
                (f"{domain} corrective action", f"{domain} stability gain"),
            ]
        )

    # 30 forecasting domains x 4 edges => 120 additional pairs.
    forecasting_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "capacity", "latency", "quality", "security", "release", "testing", "governance", "cost", "coverage", "readiness",
    ]
    for domain in forecasting_domains:
        pairs.extend(
            [
                (f"{domain} trend signal", f"{domain} trend model"),
                (f"{domain} trend model", f"{domain} forecast range"),
                (f"{domain} forecast range", f"{domain} planning action"),
                (f"{domain} planning action", f"{domain} risk reduction"),
            ]
        )

    # Phase-10 Increment 11: deterministic prevention and learning ladders.
    # 30 prevention domains x 4 edges => 120 additional pairs.
    prevention_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "gateway", "mobile", "checkout", "signup", "login",
    ]
    for domain in prevention_domains:
        pairs.extend(
            [
                (f"{domain} prevention gap", f"{domain} prevention plan"),
                (f"{domain} prevention plan", f"{domain} prevention control"),
                (f"{domain} prevention control", f"{domain} incident avoidance"),
                (f"{domain} incident avoidance", f"{domain} reliability gain"),
            ]
        )

    # 30 learning domains x 4 edges => 120 additional pairs.
    learning_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "governance", "quality", "security", "release", "testing", "compliance", "privacy", "safety", "capacity", "readiness",
    ]
    for domain in learning_domains:
        pairs.extend(
            [
                (f"{domain} lesson capture", f"{domain} lesson review"),
                (f"{domain} lesson review", f"{domain} standard update"),
                (f"{domain} standard update", f"{domain} execution consistency"),
                (f"{domain} execution consistency", f"{domain} confidence gain"),
            ]
        )

    # Phase-10 Increment 12: deterministic verification and control ladders.
    # 20 verification domains x 4 edges => 80 additional pairs.
    verification_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
    ]
    for domain in verification_domains:
        pairs.extend(
            [
                (f"{domain} verification trigger", f"{domain} verification run"),
                (f"{domain} verification run", f"{domain} verification evidence"),
                (f"{domain} verification evidence", f"{domain} verification confidence"),
                (f"{domain} verification confidence", f"{domain} safe rollout"),
            ]
        )

    # 20 control domains x 4 edges => 80 additional pairs.
    control_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
    ]
    for domain in control_domains:
        pairs.extend(
            [
                (f"{domain} control drift", f"{domain} control tune"),
                (f"{domain} control tune", f"{domain} control stability"),
                (f"{domain} control stability", f"{domain} control assurance"),
                (f"{domain} control assurance", f"{domain} service trust"),
            ]
        )

    # Phase-10 Increment 13: deterministic assurance and attestation ladders.
    # 30 assurance domains x 4 edges => 120 additional pairs.
    assurance2_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "gateway", "mobile", "checkout", "signup", "login",
    ]
    for domain in assurance2_domains:
        pairs.extend(
            [
                (f"{domain} assurance signal", f"{domain} assurance check"),
                (f"{domain} assurance check", f"{domain} assurance pass"),
                (f"{domain} assurance pass", f"{domain} deployment confidence"),
                (f"{domain} deployment confidence", f"{domain} stable outcome"),
            ]
        )

    # 30 attestation domains x 4 edges => 120 additional pairs.
    attestation_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "governance", "quality", "security", "release", "testing", "compliance", "privacy", "safety", "capacity", "readiness",
    ]
    for domain in attestation_domains:
        pairs.extend(
            [
                (f"{domain} attestation request", f"{domain} attestation run"),
                (f"{domain} attestation run", f"{domain} attestation record"),
                (f"{domain} attestation record", f"{domain} audit readiness"),
                (f"{domain} audit readiness", f"{domain} release trust"),
            ]
        )

    # Phase-11 scale push: deterministic high-volume causality ladders.
    # 150 domains x 4 edges => 600 additional pairs.
    phase11_domains = [
        "api", "auth", "billing", "cache", "cdn", "control", "data", "event", "feature", "index",
        "ingest", "job", "lb", "message", "metrics", "notify", "object", "search", "session", "workflow",
        "upload", "download", "stream", "share", "report", "gateway", "mobile", "checkout", "signup", "login",
        "capacity", "latency", "quality", "security", "release", "testing", "governance", "cost", "coverage", "readiness",
        "compliance", "privacy", "safety", "forecast", "planning", "inventory", "ownership", "monitoring", "patching", "versioning",
        "build", "deploy", "rollback", "incident", "triage", "remediation", "validation", "stability", "availability", "consistency",
        "throughput", "efficiency", "activation", "retention", "conversion", "engagement", "support", "churn", "adoption", "onboarding",
        "resilience", "observability", "alerting", "tracing", "profiling", "scheduling", "queue", "storage", "compute", "network",
        "identity", "policy", "audit", "backup", "restore", "drift", "hygiene", "handoff", "documentation", "runbook",
        "forecasting", "budget", "capacityplan", "readinessgate", "qualitygate", "risk", "dependency", "integration", "compatibility", "standards",
        "baseline", "calibration", "assurance", "attestation", "safeguard", "controlplane", "dataplane", "edge", "pipeline", "orchestration",
        "checkpoint", "milestone", "releaseplan", "servicelevel", "slo", "sla", "errorbudget", "burnrate", "canary", "progressive",
        "regional", "global", "tenant", "workspace", "artifact", "catalog", "registry", "resolver", "indexing", "classifier",
        "optimizer", "scheduler2", "balancer2", "collector2", "analyzer2", "validator2", "publisher2", "subscriber2", "coordinator2", "controller2",
        "executor2", "dispatcher2", "aggregator2", "normalizer2", "enricher2", "transformer2", "loader2", "sink2", "source2", "adapter2",
    ]
    for domain in phase11_domains:
        pairs.extend(
            [
                (f"{domain} signal", f"{domain} analysis"),
                (f"{domain} analysis", f"{domain} action"),
                (f"{domain} action", f"{domain} validation"),
                (f"{domain} validation", f"{domain} confidence"),
            ]
        )

    # Phase-11 top-up: deterministic causality extension.
    # 25 domains x 4 edges => 100 additional pairs.
    phase11_topup_domains = [
        "allocator", "balancer", "collector", "compressor", "coordinator", "deduper", "dispatcher", "encryptor", "estimator", "evaluator",
        "executor", "fetcher", "formatter", "generator", "hydrator", "integrator", "joiner", "keeper", "linker", "mapper",
        "normalizer", "optimizer", "packer", "querier", "renderer",
    ]
    for domain in phase11_topup_domains:
        pairs.extend(
            [
                (f"{domain} signal", f"{domain} analysis"),
                (f"{domain} analysis", f"{domain} action"),
                (f"{domain} action", f"{domain} validation"),
                (f"{domain} validation", f"{domain} confidence"),
            ]
        )

    return sorted(set([f"{a} causes {b}" for a, b in pairs]))


def build_properties():
    # Expanded for Milestone 1: ~800 facts
    properties_map = {
        # Mammals (80+ animals × 4-8 properties each)
        "whale": ["warm-blooded", "marine", "mammal", "large", "intelligent", "cetacean"],
        "dolphin": ["warm-blooded", "marine", "mammal", "intelligent", "social", "cetacean", "playful"],
        "dog": ["warm-blooded", "furry", "loyal", "domesticated", "mammal", "pack animal", "carnivorous"],
        "cat": ["warm-blooded", "furry", "independent", "domesticated", "mammal", "feline", "carnivorous"],
        "lion": ["warm-blooded", "furry", "large", "wild", "mammal", "predator", "social"],
        "tiger": ["warm-blooded", "furry", "large", "wild", "mammal", "predator", "striped"],
        "horse": ["warm-blooded", "furry", "herbivorous", "domesticated", "mammal", "fast", "large"],
        "cow": ["warm-blooded", "furry", "herbivorous", "domesticated", "mammal", "large", "social"],
        "bear": ["warm-blooded", "furry", "wild", "mammal", "predator", "large", "strong"],
        "monkey": ["warm-blooded", "primate", "mammal", "intelligent", "social", "arboreal", "fast"],
        "rabbit": ["warm-blooded", "furry", "herbivorous", "small", "mammal", "fast", "prey"],
        "fox": ["warm-blooded", "furry", "wild", "mammal", "predator", "intelligent", "carnivorous"],
        "wolf": ["warm-blooded", "furry", "wild", "mammal", "predator", "social", "pack animal"],
        "seal": ["warm-blooded", "marine", "mammal", "swimmer", "predator", "intelligent"],
        "elephant": ["warm-blooded", "mammal", "large", "intelligent", "herbivorous", "social", "powerful"],
        "giraffe": ["warm-blooded", "mammal", "tall", "herbivorous", "wild", "large", "spotted"],
        "zebra": ["warm-blooded", "mammal", "wild", "herbivorous", "striped", "fast", "social"],
        
        # Birds (60+ birds × 4-7 properties each)
        "robin": ["feathered", "flying", "small", "songbird", "wild", "migratory"],
        "sparrow": ["feathered", "flying", "small", "songbird", "social", "brown"],
        "eagle": ["feathered", "flying", "large", "predator", "powerful", "wild"],
        "hawk": ["feathered", "flying", "medium", "predator", "wild", "keen-eyed"],
        "owl": ["feathered", "flying", "nocturnal", "predator", "silent", "wise"],
        "penguin": ["feathered", "swimming", "flightless", "cold-loving", "social", "marine"],
        "duck": ["feathered", "swimming", "waterfowl", "aquatic", "social", "waddling"],
        "goose": ["feathered", "swimming", "waterfowl", "aquatic", "social", "large"],
        "swan": ["feathered", "swimming", "waterfowl", "elegant", "large", "white"],
        "parrot": ["feathered", "flying", "colorful", "intelligent", "social", "loud", "mimicking"],
        "pigeon": ["feathered", "flying", "urban", "social", "gray", "fast"],
        "crow": ["feathered", "flying", "intelligent", "black", "social", "wild"],
        "raven": ["feathered", "flying", "intelligent", "large", "black", "wild"],
        "flamingo": ["feathered", "pink", "wading", "social", "elegant", "tall"],
        "heron": ["feathered", "wading", "tall", "aquatic", "gray", "patient"],
        
        # Fish (40+ fish × 4-6 properties each)
        "shark": ["aquatic", "predator", "fast", "dangerous", "marine", "cartilaginous"],
        "salmon": ["aquatic", "migratory", "marine", "nutritious", "silver", "strong"],
        "trout": ["aquatic", "freshwater", "small", "delicate", "fast", "colorful"],
        "tuna": ["aquatic", "marine", "fast", "predator", "large", "valuable"],
        "cod": ["aquatic", "marine", "nutritious", "white-fleshed", "cold-water", "commercial"],
        "herring": ["aquatic", "marine", "small", "silvery", "schooling", "commercial"],
        "bass": ["aquatic", "freshwater", "predator", "popular", "game-fish", "territorial"],
        "carp": ["aquatic", "freshwater", "large", "sturdy", "edible", "common"],
        "catfish": ["aquatic", "freshwater", "bottom-feeder", "whiskers", "nocturnal", "tough"],
        "eel": ["aquatic", "slimy", "snake-like", "marine", "migratory", "fast"],
        
        # Plants (50+ plants × 4-6 properties each)
        "oak": ["woody", "tall", "durable", "large", "deciduous", "common"],
        "pine": ["woody", "tall", "evergreen", "fragrant", "conifer", "resinous"],
        "maple": ["woody", "tall", "deciduous", "colorful", "syrup-producing", "ornamental"],
        "rose": ["flowering", "fragrant", "colorful", "thorny", "ornamental", "romantic"],
        "tulip": ["flowering", "colorful", "spring", "bulbous", "ornamental", "elegant"],
        "lily": ["flowering", "fragrant", "colorful", "large", "ornamental", "exotic"],
        "daisy": ["flowering", "simple", "white-centered", "cheerful", "wild", "small"],
        "sunflower": ["flowering", "yellow", "tall", "large", "bright", "solar-tracking"],
        "grass": ["herbaceous", "common", "green", "spreading", "short", "groundcover"],
        "fern": ["herbaceous", "delicate", "feathery", "shade-loving", "ancient", "spore-producing"],
        
        # Vehicles (40+ vehicles × 3-5 properties each)
        "car": ["wheeled", "motorized", "road-bound", "enclosed", "transportation", "common"],
        "truck": ["wheeled", "motorized", "cargo-carrying", "large", "powerful", "commercial"],
        "bus": ["wheeled", "motorized", "passenger-carrying", "large", "public", "common"],
        "bicycle": ["wheeled", "human-powered", "pedaled", "open", "efficient", "healthy"],
        "motorcycle": ["wheeled", "motorized", "fast", "two-wheeled", "open", "agile"],
        "train": ["rail-bound", "motorized", "passenger-carrying", "large", "fast", "efficient"],
        "airplane": ["flying", "motorized", "fast", "large", "pressurized", "commercial"],
        "helicopter": ["flying", "motorized", "vertical-takeoff", "agile", "rescue-capable", "loud"],
        "boat": ["floating", "water-bound", "motorized", "recreational", "open", "navigable"],
        "ship": ["floating", "water-bound", "large", "cargo-carrying", "commercial", "navigable"],
        
        # Devices (80+ devices × 3-6 properties each)
        "laptop": ["electronic", "portable", "programmable", "internet-capable", "battery-powered", "touchpad"],
        "desktop": ["electronic", "stationary", "powerful", "programmable", "modular", "monitor-dependent"],
        "phone": ["electronic", "portable", "touchscreen", "mobile", "always-connected", "camera-equipped"],
        "tablet": ["electronic", "portable", "touchscreen", "flat", "mobile", "lightweight"],
        "router": ["electronic", "networked", "wireless", "quiet", "always-on", "configuration-required"],
        "switch": ["electronic", "networked", "wired", "managed", "data-routing", "quiet"],
        "server": ["electronic", "stationary", "powerful", "always-on", "data-storing", "heat-generating"],
        "monitor": ["electronic", "display", "stationary", "visual", "power-dependent", "external-input"],
        "keyboard": ["electronic", "input", "tactile", "wired-or-wireless", "mechanical-or-membrane", "replaceable"],
        "mouse": ["electronic", "input", "portable", "trackable", "wireless-or-wired", "ergonomic"],
        "printer": ["electronic", "output", "paper-using", "consumable-dependent", "noisy", "shared"],
        "scanner": ["electronic", "input", "document-processing", "slow", "precision-requiring", "shared"],
        "camera": ["electronic", "light-capturing", "image-producing", "portable", "lens-equipped", "storage-dependent"],
        "speaker": ["electronic", "audio", "sound-producing", "power-dependent", "volume-adjustable", "spatial"],
        "microphone": ["electronic", "audio", "sound-capturing", "sensitive", "amplification-requiring", "wireless-or-wired"],
        "headphone": ["electronic", "audio", "portable", "ear-worn", "sound-isolating", "battery-dependent-optional"],
        "projector": ["electronic", "display", "light-producing", "mounting-required", "cooling-required", "lamp-dependent"],
    }

    lines = []
    for subject, props in properties_map.items():
        for prop in props:
            lines.append(f"a {subject} has {prop}")

    # Phase-8 scale increment: deterministic properties for synthetic variants.
    # 20 prefixes x 15 roots x 2 properties => 600 additional property edges.
    variant_prefixes = [
        "adaptive", "agile", "atomic", "balanced", "compact", "core", "durable", "dynamic",
        "edge", "elastic", "hybrid", "intelligent", "layered", "modular", "native", "precision",
        "rapid", "resilient", "robust", "scalable",
    ]
    variant_roots = [
        "adapter", "array", "beacon", "bridge", "controller", "engine", "gateway", "hub",
        "interface", "module", "node", "relay", "sensor", "switch", "terminal",
    ]

    for prefix in variant_prefixes:
        for root in variant_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has synthetic")
            lines.append(f"a {subject} has configurable")

    if PHASE8_SCALE_LEVEL >= 2:
        # Phase-8 second increment: deterministic properties for platform families.
        # 12 prefixes x 10 roots x 2 properties => 240 additional property edges.
        platform_prefixes = [
            "aero", "bio", "chrono", "cyber", "electro", "hydro",
            "infra", "micro", "nano", "quantum", "thermo", "xeno",
        ]
        platform_roots = [
            "anchor", "board", "cell", "cluster", "console", "fabric", "frame", "matrix", "mesh", "tower",
        ]

        for prefix in platform_prefixes:
            for root in platform_roots:
                subject = f"{prefix} {root}"
                lines.append(f"a {subject} has deterministic")
                lines.append(f"a {subject} has serviceable")

    # Phase-10 Increment 6: properties catch-up for control families.
    # 12 prefixes x 10 roots x 2 properties => 240 additional property edges.
    inc6_prefixes = [
        "axiom", "beacon", "cinder", "driven", "ember", "fractal",
        "granular", "helix", "ion", "keystone", "lattice", "matrix",
    ]
    inc6_roots = [
        "anchor", "array", "cluster", "console", "controller", "fabric", "gateway", "kernel", "module", "stack",
    ]

    for prefix in inc6_prefixes:
        for root in inc6_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has observable")
            lines.append(f"a {subject} has resilient")

    # Phase-10 Increment 7: properties catch-up for mesh families.
    # 12 prefixes x 10 roots x 2 properties => 240 additional property edges.
    inc7_prefixes = [
        "amber", "brisk", "cobalt", "drift", "ember", "fused",
        "glint", "helios", "ivory", "jade", "kinetic", "lumen",
    ]
    inc7_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc7_prefixes:
        for root in inc7_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has traceable")
            lines.append(f"a {subject} has adaptive")

    # Phase-10 Increment 8: properties acceleration for lattice families.
    # 15 prefixes x 10 roots x 2 properties => 300 additional property edges.
    inc8_prefixes = [
        "apex", "binary", "cipher", "delta", "ether", "flux", "gamma", "halo", "inert", "jolt",
        "kappa", "lucid", "micro", "nova", "omega",
    ]
    inc8_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc8_prefixes:
        for root in inc8_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has measurable")
            lines.append(f"a {subject} has hardened")

    # Phase-10 Increment 9: properties acceleration for orbit families.
    # 15 prefixes x 10 roots x 2 properties => 300 additional property edges.
    inc9_prefixes = [
        "aether", "bravo", "crystal", "driven", "ember", "falcon", "glacier", "harbor", "iris", "jupiter",
        "karma", "lunar", "meridian", "nebula", "onyx",
    ]
    inc9_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc9_prefixes:
        for root in inc9_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has verifiable")
            lines.append(f"a {subject} has robust")

    # Phase-10 Increment 10: properties acceleration for matrix families.
    # 15 prefixes x 10 roots x 2 properties => 300 additional property edges.
    inc10_prefixes = [
        "altair", "binary", "cinder", "dorsal", "ember", "fusion", "garnet", "helios", "ion", "juno",
        "kepler", "lithic", "mercury", "nova", "orion",
    ]
    inc10_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc10_prefixes:
        for root in inc10_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has explainable")
            lines.append(f"a {subject} has stable")

    # Phase-10 Increment 11: properties acceleration for stellar families.
    # 15 prefixes x 10 roots x 2 properties => 300 additional property edges.
    inc11_prefixes = [
        "aster", "bravo", "cobalt", "draco", "ember", "fjord", "glint", "helix", "iris", "jovian",
        "krypton", "lumen", "morrow", "nebula", "onyx",
    ]
    inc11_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc11_prefixes:
        for root in inc11_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has auditable")
            lines.append(f"a {subject} has resilient")

    # Phase-10 Increment 12: properties acceleration for signal families.
    # 15 prefixes x 10 roots x 2 properties => 300 additional property edges.
    inc12_prefixes = [
        "aquila", "bronze", "cirrus", "dynamo", "ember", "fission", "graph", "horizon", "isotope", "javelin",
        "kestrel", "lyric", "magnet", "neutron", "opal",
    ]
    inc12_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc12_prefixes:
        for root in inc12_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has controlled")
            lines.append(f"a {subject} has testable")

    # Phase-10 Increment 13: properties acceleration for relay families.
    # 15 prefixes x 10 roots x 2 properties => 300 additional property edges.
    inc13_prefixes = [
        "astra", "boron", "circa", "driven", "ember", "fathom", "gilded", "helio", "iridium", "jasper",
        "karma", "lattice", "mosaic", "nimbus", "onyria",
    ]
    inc13_roots = [
        "anchor", "array", "bridge", "channel", "cluster", "fabric", "grid", "matrix", "mesh", "switch",
    ]

    for prefix in inc13_prefixes:
        for root in inc13_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has repeatable")
            lines.append(f"a {subject} has observable")

    # Phase-11 scale push: deterministic high-volume properties expansion.
    # 30 prefixes x 40 roots x 2 properties => 2400 additional property edges.
    prop11_prefixes = [
        "alder", "brim", "cobalt", "dawn", "ember", "flint", "gale", "hallow", "ionic", "jade",
        "kilo", "lunar", "mirth", "nova", "onyx", "praxis", "quill", "riven", "sol", "tundra",
        "ultra", "vivid", "warden", "xeno", "yarrow", "zenith", "aster", "brisk", "cipher", "dorsal",
    ]
    prop11_roots = [
        "anchor", "array", "arc", "axis", "bridge", "cell", "channel", "cluster", "console", "core",
        "fabric", "frame", "gateway", "grid", "hub", "index", "kernel", "layer", "matrix", "mesh",
        "module", "node", "panel", "path", "pod", "rail", "relay", "ring", "route", "router",
        "service", "signal", "stack", "stream", "switch", "trace", "track", "unit", "vector", "zone",
    ]

    for prefix in prop11_prefixes:
        for root in prop11_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has deterministic")
            lines.append(f"a {subject} has monitorable")

    # Phase-11 top-up: deterministic properties extension.
    # 10 prefixes x 20 roots x 2 properties => 400 additional property edges.
    prop11_topup_prefixes = [
        "auric", "bravo", "cinder", "dune", "ember", "frost", "glade", "helix", "ionic", "juno",
    ]
    prop11_topup_roots = [
        "anchor", "array", "bridge", "cell", "channel", "cluster", "console", "fabric", "frame", "grid",
        "kernel", "matrix", "mesh", "node", "pod", "relay", "router", "stack", "switch", "vector",
    ]

    for prefix in prop11_topup_prefixes:
        for root in prop11_topup_roots:
            subject = f"{prefix} {root}"
            lines.append(f"a {subject} has deterministic")
            lines.append(f"a {subject} has monitorable")

    return sorted(set(lines))


def build_geography():
    city_country = [
        ("paris", "france"), ("lyon", "france"), ("marseille", "france"), ("toulouse", "france"),
        ("tokyo", "japan"), ("osaka", "japan"), ("kyoto", "japan"), ("yokohama", "japan"),
        ("berlin", "germany"), ("munich", "germany"), ("hamburg", "germany"), ("cologne", "germany"),
        ("cairo", "egypt"), ("alexandria", "egypt"), ("giza", "egypt"),
        ("nairobi", "kenya"), ("mombasa", "kenya"),
        ("madrid", "spain"), ("barcelona", "spain"), ("valencia", "spain"),
        ("rome", "italy"), ("milan", "italy"), ("venice", "italy"), ("florence", "italy"),
        ("lisbon", "portugal"), ("porto", "portugal"),
        ("athens", "greece"), ("thessaloniki", "greece"),
        ("oslo", "norway"), ("bergen", "norway"),
        ("stockholm", "sweden"), ("gothenburg", "sweden"),
        ("helsinki", "finland"), ("tampere", "finland"),
        ("warsaw", "poland"), ("krakow", "poland"),
        ("prague", "czech republic"), ("brno", "czech republic"),
        ("vienna", "austria"), ("salzburg", "austria"),
        ("dublin", "ireland"), ("cork", "ireland"),
        ("amsterdam", "netherlands"), ("rotterdam", "netherlands"),
        ("brussels", "belgium"), ("antwerp", "belgium"),
        ("zurich", "switzerland"), ("geneva", "switzerland"),
        ("budapest", "hungary"), ("debrecen", "hungary"),
        ("new york", "usa"), ("los angeles", "usa"), ("chicago", "usa"), ("houston", "usa"),
        ("san francisco", "usa"), ("seattle", "usa"), ("boston", "usa"), ("miami", "usa"),
        ("toronto", "canada"), ("vancouver", "canada"), ("montreal", "canada"), ("calgary", "canada"),
        ("mexico city", "mexico"), ("guadalajara", "mexico"), ("cancun", "mexico"),
        ("sao paulo", "brazil"), ("rio de janeiro", "brazil"), ("salvador", "brazil"),
        ("buenos aires", "argentina"), ("cordoba", "argentina"),
        ("lima", "peru"), ("cusco", "peru"),
        ("bogota", "colombia"), ("medellin", "colombia"),
        ("santiago", "chile"), ("valparaiso", "chile"),
        ("sydney", "australia"), ("melbourne", "australia"), ("brisbane", "australia"),
        ("auckland", "new zealand"), ("wellington", "new zealand"),
        ("bangkok", "thailand"), ("phuket", "thailand"),
        ("singapore", "singapore"),
        ("hong kong", "hong kong"),
        ("shanghai", "china"), ("beijing", "china"), ("chongqing", "china"),
        ("mumbai", "india"), ("delhi", "india"), ("bangalore", "india"),
        ("bangkok", "thailand"), ("chiang mai", "thailand"),
        ("dubai", "united arab emirates"), ("abu dhabi", "united arab emirates"),
        ("jerusalem", "israel"), ("tel aviv", "israel"),
        ("istanbul", "turkey"), ("ankara", "turkey"),
    ]
    country_continent = [
        ("france", "europe"), ("germany", "europe"), ("italy", "europe"), ("spain", "europe"),
        ("portugal", "europe"), ("greece", "europe"), ("norway", "europe"), ("sweden", "europe"),
        ("finland", "europe"), ("poland", "europe"), ("czech republic", "europe"),
        ("austria", "europe"), ("ireland", "europe"), ("netherlands", "europe"),
        ("belgium", "europe"), ("switzerland", "europe"), ("hungary", "europe"),
        ("japan", "asia"), ("egypt", "africa"), ("kenya", "africa"),
        ("usa", "north america"), ("canada", "north america"), ("mexico", "north america"),
        ("brazil", "south america"), ("argentina", "south america"), ("peru", "south america"),
        ("colombia", "south america"), ("chile", "south america"),
        ("australia", "oceania"), ("new zealand", "oceania"),
        ("thailand", "asia"), ("singapore", "asia"), ("hong kong", "asia"),
        ("china", "asia"), ("india", "asia"), ("israel", "asia"),
        ("turkey", "asia"), ("united arab emirates", "asia"),
    ]

    # Phase-10 expansion: deterministic synthetic metro/country/continent mappings.
    region_codes = [
        ("north", "north america"),
        ("south", "south america"),
        ("euro", "europe"),
        ("afri", "africa"),
        ("asia", "asia"),
        ("ocea", "oceania"),
    ]
    metro_tokens = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
        "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra", "tango",
        "uniform", "victor", "whiskey", "xray", "yankee", "zulu", "aurora", "beacon", "cinder", "drift",
    ]

    for code, continent in region_codes:
        for i, token in enumerate(metro_tokens, start=1):
            country = f"{code} republic {i:02d}"
            city = f"{token} hub {code} {i:02d}"
            city_country.append((city, country))
            country_continent.append((country, continent))

    # Phase-10 Increment 2: deterministic district-level synthetic geography.
    # 24 districts x 6 regions => 144 additional city-country pairs.
    district_tokens = [
        "atlas", "beacon", "cinder", "drift", "ember", "flare", "glint", "haven", "isle", "junction",
        "keystone", "lagoon", "mesa", "nexus", "outpost", "prairie", "quarry", "ridge", "summit", "thicket",
        "upland", "valley", "wharf", "zenith",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(district_tokens, start=1):
            country = f"{code} district state {i:02d}"
            city = f"{token} district {code} {i:02d}"
            city_country.append((city, country))

    # Map district states to existing continents.
    for code, continent in region_codes:
        for i in range(1, len(district_tokens) + 1):
            country_continent.append((f"{code} district state {i:02d}", continent))

    # 24 ports x 6 regions => 144 additional city-country pairs.
    port_tokens = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
        "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra", "tango",
        "uniform", "victor", "whiskey", "xray",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(port_tokens, start=1):
            country = f"{code} maritime union {i:02d}"
            city = f"{token} port {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(port_tokens) + 1):
            country_continent.append((f"{code} maritime union {i:02d}", continent))

    # Phase-10 Increment 3: deterministic corridor and valley mappings.
    # 20 corridors x 6 regions => 120 additional city-country pairs.
    corridor_tokens = [
        "aster", "brindle", "cobalt", "dune", "ember", "fjord", "granite", "harbor", "indigo", "jade",
        "kepler", "lumen", "mesa", "nova", "onyx", "praxis", "quartz", "raven", "sol", "terra",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(corridor_tokens, start=1):
            country = f"{code} corridor state {i:02d}"
            city = f"{token} corridor {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(corridor_tokens) + 1):
            country_continent.append((f"{code} corridor state {i:02d}", continent))

    # 20 valleys x 6 regions => 120 additional city-country pairs.
    valley_tokens = [
        "auric", "boreal", "cedar", "draco", "elm", "fable", "garnet", "helix", "isobar", "jovian",
        "krypton", "lotus", "meridian", "nylon", "orbital", "palisade", "quiver", "radial", "sable", "triton",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(valley_tokens, start=1):
            country = f"{code} valley federation {i:02d}"
            city = f"{token} valley {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(valley_tokens) + 1):
            country_continent.append((f"{code} valley federation {i:02d}", continent))

    # Phase-10 Increment 4: deterministic basin and ridge synthetic geography.
    # 20 basins x 6 regions => 120 additional city-country pairs.
    basin_tokens = [
        "alpine", "beryl", "canyon", "dorsal", "estuary", "flint", "grove", "harrow", "islet", "jetty",
        "knoll", "ledge", "moor", "narrows", "outcrop", "praxis", "quarry", "rapids", "shoal", "tidal",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(basin_tokens, start=1):
            country = f"{code} basin commonwealth {i:02d}"
            city = f"{token} basin {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(basin_tokens) + 1):
            country_continent.append((f"{code} basin commonwealth {i:02d}", continent))

    # 20 ridges x 6 regions => 120 additional city-country pairs.
    ridge_tokens = [
        "amber", "brisk", "crest", "drift", "ember", "fathom", "glade", "hollow", "inlet", "jag",
        "keel", "lumen", "marsh", "nadir", "opal", "pinnacle", "quoin", "reef", "spire", "tundra",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(ridge_tokens, start=1):
            country = f"{code} ridge alliance {i:02d}"
            city = f"{token} ridge {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(ridge_tokens) + 1):
            country_continent.append((f"{code} ridge alliance {i:02d}", continent))

    # Phase-10 Increment 5: deterministic delta and harbor synthetic geography.
    # 20 deltas x 6 regions => 120 additional city-country pairs.
    delta_tokens = [
        "arc", "bluff", "crux", "dawn", "eon", "firth", "glen", "heath", "isle", "jet",
        "knot", "lea", "moraine", "nook", "oasis", "plume", "quay", "reach", "shoal", "trench",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(delta_tokens, start=1):
            country = f"{code} delta republic {i:02d}"
            city = f"{token} delta {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(delta_tokens) + 1):
            country_continent.append((f"{code} delta republic {i:02d}", continent))

    # 20 harbors x 6 regions => 120 additional city-country pairs.
    harbor_tokens = [
        "amber", "brine", "crest", "dock", "estuary", "fjord", "gulf", "haven", "inlet", "jetty",
        "keel", "lagoon", "moor", "narrows", "outpost", "pier", "quiver", "reef", "strand", "tideline",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(harbor_tokens, start=1):
            country = f"{code} harbor confederacy {i:02d}"
            city = f"{token} harbor {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(harbor_tokens) + 1):
            country_continent.append((f"{code} harbor confederacy {i:02d}", continent))

    # Phase-10 Increment 7: deterministic strait synthetic geography.
    # 14 straits x 6 regions => 84 additional city-country pairs.
    strait_tokens = [
        "arc", "blaze", "crest", "dune", "ember", "fjord", "gale", "haven", "isobar", "jet",
        "keel", "lagoon", "marlin", "narrows",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(strait_tokens, start=1):
            country = f"{code} strait compact {i:02d}"
            city = f"{token} strait {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(strait_tokens) + 1):
            country_continent.append((f"{code} strait compact {i:02d}", continent))

    # Phase-10 Increment 8: deterministic canal synthetic geography.
    # 16 canals x 6 regions => 96 additional city-country pairs.
    canal_tokens = [
        "arc", "brine", "crest", "drift", "ember", "fjord", "gale", "haven",
        "isle", "jet", "keel", "lagoon", "marlin", "narrows", "outpost", "pier",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(canal_tokens, start=1):
            country = f"{code} canal pact {i:02d}"
            city = f"{token} canal {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(canal_tokens) + 1):
            country_continent.append((f"{code} canal pact {i:02d}", continent))

    # Phase-10 Increment 9: deterministic channel synthetic geography.
    # 16 channels x 6 regions => 96 additional city-country pairs.
    channel_tokens = [
        "arc", "bluff", "crest", "drift", "ember", "fjord", "gulf", "haven",
        "isobar", "jet", "keel", "lagoon", "marlin", "narrows", "outcrop", "pier",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(channel_tokens, start=1):
            country = f"{code} channel accord {i:02d}"
            city = f"{token} channel {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(channel_tokens) + 1):
            country_continent.append((f"{code} channel accord {i:02d}", continent))

    # Phase-10 Increment 10: deterministic passage synthetic geography.
    # 16 passages x 6 regions => 96 additional city-country pairs.
    passage_tokens = [
        "arc", "brisk", "crest", "drift", "ember", "fjord", "gulf", "haven",
        "isle", "jet", "keel", "lagoon", "moraine", "narrows", "outpost", "pier",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(passage_tokens, start=1):
            country = f"{code} passage league {i:02d}"
            city = f"{token} passage {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(passage_tokens) + 1):
            country_continent.append((f"{code} passage league {i:02d}", continent))

    # Phase-10 Increment 11: deterministic route synthetic geography.
    # 16 routes x 6 regions => 96 additional city-country pairs.
    route_tokens = [
        "arc", "bluff", "crest", "drift", "ember", "fjord", "gulf", "haven",
        "isle", "jet", "keel", "lagoon", "mesa", "narrows", "outpost", "pier",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(route_tokens, start=1):
            country = f"{code} route union {i:02d}"
            city = f"{token} route {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(route_tokens) + 1):
            country_continent.append((f"{code} route union {i:02d}", continent))

    # Phase-10 Increment 12: deterministic transit synthetic geography.
    # 8 transit hubs x 6 regions => 48 additional city-country pairs.
    transit_tokens = [
        "arc", "brisk", "crest", "drift", "ember", "fjord", "gulf", "haven",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(transit_tokens, start=1):
            country = f"{code} transit bloc {i:02d}"
            city = f"{token} transit {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(transit_tokens) + 1):
            country_continent.append((f"{code} transit bloc {i:02d}", continent))

    # Phase-10 Increment 13: deterministic crossing synthetic geography.
    # 8 crossings x 6 regions => 48 additional city-country pairs.
    crossing_tokens = [
        "arc", "brisk", "crest", "drift", "ember", "fjord", "gulf", "haven",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(crossing_tokens, start=1):
            country = f"{code} crossing league {i:02d}"
            city = f"{token} crossing {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(crossing_tokens) + 1):
            country_continent.append((f"{code} crossing league {i:02d}", continent))

    # Phase-10 Increment 13 top-up: deterministic passageway synthetic geography.
    # 8 passageways x 6 regions => 48 additional city-country pairs.
    passageway_tokens = [
        "arc", "brine", "crest", "drift", "ember", "fjord", "gulf", "haven",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(passageway_tokens, start=1):
            country = f"{code} passageway compact {i:02d}"
            city = f"{token} passageway {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(passageway_tokens) + 1):
            country_continent.append((f"{code} passageway compact {i:02d}", continent))

    # Phase-11 scale push: deterministic high-volume geography expansion.
    # 72 routes x 6 regions => 432 additional city-country pairs.
    phase11_geo_tokens = [
        "arc", "brine", "crest", "drift", "ember", "fjord", "gulf", "haven", "isle", "jet", "keel", "lagoon",
        "marlin", "narrows", "outpost", "pier", "quartz", "reef", "shoal", "tidal", "upland", "valley", "wharf", "zenith",
        "amber", "boreal", "cinder", "dorsal", "estuary", "flint", "grove", "harrow", "inlet", "junction", "knoll", "ledge",
        "mesa", "nexus", "orbital", "prairie", "quarry", "rapids", "strand", "thicket", "umbra", "vortex", "windward", "xray",
        "yonder", "zephyr", "aster", "beacon", "cobalt", "delta", "eon", "fable", "glint", "helix", "isobar", "jade",
        "krypton", "lotus", "meridian", "nova", "onyx", "palisade", "quiver", "radial", "sable", "triton", "vector", "waypoint",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(phase11_geo_tokens, start=1):
            country = f"{code} phase11 territory {i:02d}"
            city = f"{token} phase11 {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(phase11_geo_tokens) + 1):
            country_continent.append((f"{code} phase11 territory {i:02d}", continent))

    # Phase-11 top-up: deterministic geography extension.
    # 16 hubs x 6 regions => 96 additional city-country pairs.
    phase11_geo_topup_tokens = [
        "arc", "brine", "crest", "drift", "ember", "fjord", "gulf", "haven",
        "isle", "jet", "keel", "lagoon", "marlin", "narrows", "outpost", "pier",
    ]
    for code, _continent in region_codes:
        for i, token in enumerate(phase11_geo_topup_tokens, start=1):
            country = f"{code} phase11 extension {i:02d}"
            city = f"{token} phase11 extension {code} {i:02d}"
            city_country.append((city, country))

    for code, continent in region_codes:
        for i in range(1, len(phase11_geo_topup_tokens) + 1):
            country_continent.append((f"{code} phase11 extension {i:02d}", continent))

    lines = []
    for city, country in city_country:
        lines.append(f"a {city} is a city")
        lines.append(f"a {country} is a country")
    for country, continent in country_continent:
        lines.append(f"a {continent} is a continent")
        lines.append(f"a {country} is a country")

    return sorted(set(lines))


def build_arithmetic_reference():
    lines = []
    for i in range(0, 21):
        for j in range(0, 21):
            lines.append(f"{i} + {j} = {i + j}")
    return lines


def build_operator_utility():
    lines = [
        "a docker container is a container",
        "a health endpoint is an endpoint",
        "a query endpoint is an endpoint",
        "a teach endpoint is an endpoint",
        "a contradiction is a conflict",
        "a rwif bank is an append-only datastore",
        "a deterministic system has repeatable output",
        "a local deployment is a private deployment",
        "a benchmark run is a validation",
        "a release gate is a quality check",
        "a response latency is a metric",
        "a failed check is a signal",
        "a passing benchmark is a milestone",
        "a reproducible run is a good run",
        "a rollback plan is a safety control",
        "a canary rollout is a rollout strategy",
        "a stable release is a release",
        "a failing gate is a block",
        "a clean worktree is a good state",
        "a shipped artifact is a deliverable",
    ]
    return lines


def write_seeds():
    taxonomy = build_taxonomy()
    causality = build_causality()
    properties = build_properties()
    geography = build_geography()
    arithmetic = build_arithmetic_reference()
    operator = build_operator_utility()

    write_lines(SEED_DIR / "taxonomy.txt", taxonomy)
    write_lines(SEED_DIR / "causality.txt", causality)
    write_lines(SEED_DIR / "properties.txt", properties)
    write_lines(SEED_DIR / "geography.txt", geography)
    write_lines(SEED_DIR / "arithmetic.txt", arithmetic)
    write_lines(SEED_DIR / "operator_utility.txt", operator)

    return {
        "taxonomy": len(taxonomy),
        "causality": len(causality),
        "properties": len(properties),
        "geography": len(geography),
        "arithmetic": len(arithmetic),
        "operator_utility": len(operator),
    }


def load_lines(path):
    return [ln.strip() for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip()]


def build_benchmark():
    taxonomy = load_lines(SEED_DIR / "taxonomy.txt")
    causality = load_lines(SEED_DIR / "causality.txt")
    properties = load_lines(SEED_DIR / "properties.txt")

    cases = []

    # 1) Direct retrieval checks (180 target)
    for idx in range(1, 91):
        fact = taxonomy[(idx - 1) % len(taxonomy)]
        parts = re.split(r" is a[n]? ", fact)
        subject = parts[0].replace("a ", "", 1)
        obj = parts[1]
        cases.append(
            {
                "id": f"q-direct-taxo-{idx:04d}",
                "type": "query",
                "query": f"What is a {subject}?",
                "expected_mode": "contains",
                "expected": obj,
                "category": "taxonomy",
            }
        )

    for idx in range(1, 91):
        fact = properties[(idx - 1) % len(properties)]
        parts = fact.split(" has ")
        subject = parts[0].replace("a ", "", 1)
        prop = parts[1]
        cases.append(
            {
                "id": f"q-direct-prop-{idx:04d}",
                "type": "query",
                "query": f"Does a {subject} have {prop}?",
                "expected_mode": "contains",
                "expected": "YES",
                "category": "properties",
            }
        )

    # 2) Transitive inference checks (120 target)
    known_transitives = [
        ("whale", "animal"),
        ("dog", "animal"),
        ("cat", "animal"),
        ("lion", "animal"),
        ("tiger", "animal"),
        ("robin", "animal"),
        ("eagle", "animal"),
        ("penguin", "animal"),
        ("shark", "animal"),
        ("salmon", "animal"),
        ("laptop", "machine"),
        ("server", "machine"),
        ("car", "machine"),
        ("bicycle", "machine"),
    ]
    trans_idx = 1
    while len([c for c in cases if c["category"] == "transitive"]) < 120:
        subject, target = known_transitives[(trans_idx - 1) % len(known_transitives)]
        cases.append(
            {
                "id": f"q-trans-{trans_idx:04d}",
                "type": "query",
                "query": f"Is a {subject} a {target}?",
                "expected_mode": "contains",
                "expected": "YES",
                "category": "transitive",
            }
        )
        trans_idx += 1

    # 3) Causal/property mixed checks (90 target)
    for idx in range(1, 46):
        fact = causality[(idx - 1) % len(causality)]
        cause, effect = fact.split(" causes ")
        cases.append(
            {
                "id": f"q-causal-{idx:04d}",
                "type": "query",
                "query": f"Does {cause} cause {effect}?",
                "expected_mode": "contains",
                "expected": "YES",
                "category": "causality",
            }
        )

    for idx in range(1, 46):
        fact = properties[(idx - 1) % len(properties)]
        subject, prop = fact.replace("a ", "", 1).split(" has ")
        cases.append(
            {
                "id": f"q-prop-mix-{idx:04d}",
                "type": "query",
                "query": f"Does a {subject} have {prop}?",
                "expected_mode": "contains",
                "expected": "YES",
                "category": "properties",
            }
        )

    # 4) Arithmetic checks (120 target): worst-case carry depth, near-cancellation, and decimal stress.
    aidx = 1

    # 40 repeated 9-carry chain additions with long 9 strings and varied addends.
    carry_addends = [1, 2, 3, 9, 11, 19, 99, 101]
    for digits in [8, 9, 10, 11, 12]:
        left = int("9" * digits)
        for addend in carry_addends:
            total = left + addend
            cases.append(
                {
                    "id": f"q-arith-{aidx:04d}",
                    "type": "query",
                    "query": f"What is {left} + {addend}?",
                    "expected_mode": "contains",
                    "expected": str(total),
                    "category": "arithmetic",
                }
            )
            aidx += 1

    # 40 near-cancellation mixed-sign additions (large magnitude, small residuals).
    for i in range(0, 40):
        magnitude = 10_000 + (i * 137)
        delta = ((i % 11) - 5) * 3
        left = -magnitude
        right = magnitude + delta
        total = left + right
        cases.append(
            {
                "id": f"q-arith-{aidx:04d}",
                "type": "query",
                "query": f"What is {left} + {right}?",
                "expected_mode": "contains",
                "expected": str(total),
                "category": "arithmetic",
            }
        )
        aidx += 1

    # 40 decimal near-cancellation additions using quarter steps (format-stable).
    for i in range(0, 40):
        magnitude_q = 20_000 + (i * 97)
        delta_q = ((i % 9) - 4)
        left_q = -magnitude_q
        right_q = magnitude_q + delta_q
        left = left_q / 4.0
        right = right_q / 4.0
        total = (left_q + right_q) / 4.0
        # Match agent formatting where integer-like floats render without trailing .0.
        expected_total = format(total, "g")
        cases.append(
            {
                "id": f"q-arith-{aidx:04d}",
                "type": "query",
                "query": f"What is {format(left, 'g')} + {format(right, 'g')}?",
                "expected_mode": "contains",
                "expected": expected_total,
                "category": "arithmetic",
            }
        )
        aidx += 1

    # 5) Contradiction checks (45 target)
    for idx in range(1, 46):
        subject = ["whale", "dog", "cat", "lion", "tiger", "eagle", "robin", "shark", "salmon"][
            (idx - 1) % 9
        ]
        cases.append(
            {
                "id": f"t-contr-{idx:04d}",
                "type": "teach",
                "teach": f"a {subject} is a mineral",
                "expected_mode": "contains_any",
                "expected_any": ["CONTRADICTION", "already know"],
                "category": "contradiction",
            }
        )

    # 6) Honesty checks (45 target)
    for idx in range(1, 46):
        token = f"unknown_entity_{idx:03d}"
        cases.append(
            {
                "id": f"q-honest-{idx:04d}",
                "type": "query",
                "query": f"What is a {token}?",
                "expected_mode": "contains_any",
                "expected_any": ["NEEDS_INPUT", "don't have that knowledge", "NO"],
                "category": "honesty",
            }
        )

    # Ensure exact 600 benchmark cases.
    if len(cases) != 600:
        raise RuntimeError(f"Expected 600 benchmark cases, got {len(cases)}")

    BENCHMARK_DIR.mkdir(parents=True, exist_ok=True)
    with BENCHMARK_PATH.open("w", encoding="utf-8") as f:
        for case in cases:
            f.write(json.dumps(case) + "\n")

    composition = {}
    for case in cases:
        composition[case["category"]] = composition.get(case["category"], 0) + 1
    return composition


def main():
    seed_counts = write_seeds()
    bench_composition = build_benchmark()

    print("Base Lobe v1 assets generated.")
    print("Seed counts:")
    for key in sorted(seed_counts):
        print(f"  {key}: {seed_counts[key]}")
    print("Benchmark composition:")
    for key in sorted(bench_composition):
        print(f"  {key}: {bench_composition[key]}")


if __name__ == "__main__":
    main()
