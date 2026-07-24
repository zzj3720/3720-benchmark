#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "UnityPy==1.25.2",
#   "TypeTreeGeneratorAPI==0.0.10",
# ]
# ///
"""Extract gameplay-only campaign data from an owned Overcooked installation."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
import zlib
from pathlib import Path
from typing import Any

import UnityPy
from UnityPy.classes.PPtr import PPtr
from UnityPy.helpers.TypeTreeGenerator import TypeTreeGenerator


APP_ID = "448510"
DEPOT_MANIFEST = "9147951660691927483"
UNITY_VERSION = "5.5.0p1"
SOURCE_HASHES = {
    "Managed/Assembly-CSharp.dll": (
        "29369a9a67ac77fb55871e269fbf1a2d21d595650f1b6d2fd2fb146810b8f284"
    ),
    "globalgamemanagers": (
        "59e2421d3eac651d392201503731e375f2bebe4c9bd0b2bcf58925d03eb1d047"
    ),
    "resources.assets": (
        "08d2ac677fce27cc3ee1aba932d6bb08247124e10fc622412cb3334bb9bdaef8"
    ),
    "sharedassets0.assets": (
        "e5999349077874248c55ef76fa1852fbca3a5003272defb25e5f1f51b7d74f80"
    ),
    "sharedassets2.assets": (
        "c1838ef656f6b89d56ccf4a9c48d9a24486080c8056d78be15c3a750177b8dc6"
    ),
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def json_bytes(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, separators=(",", ":")) + "\n"
    ).encode()


def locate_data_root(explicit: Path | None) -> Path:
    candidates: list[Path] = []
    if explicit:
        candidates.extend(
            [
                explicit,
                explicit / "Overcooked_Data",
                explicit / "windows/Overcooked_Data",
            ]
        )
    candidates.append(
        Path.home() / ".3720/game-sources/overcooked-448510/windows/Overcooked_Data"
    )
    for candidate in candidates:
        if (candidate / "Managed/Assembly-CSharp.dll").is_file():
            return candidate.resolve()
    raise SystemExit(
        "Overcooked_Data not found; pass --game-root pointing to the owned "
        "Windows installation or its Overcooked_Data directory"
    )


def verify_source(data_root: Path) -> None:
    actual = {name: sha256(data_root / name) for name in SOURCE_HASHES}
    if actual != SOURCE_HASHES:
        raise SystemExit(
            "unsupported Overcooked build; expected Steam depot manifest "
            f"{DEPOT_MANIFEST}, got hashes {actual}"
        )


def asset(environment: Any, basename: str) -> Any:
    for candidate in environment.assets:
        name = str(getattr(candidate, "name", "")).replace("\\", "/").rsplit("/", 1)[-1]
        if name == basename:
            return candidate
    raise ValueError(f"serialized asset {basename!r} was not loaded")


def mono(reader: Any) -> tuple[str, dict[str, Any]]:
    head = reader.parse_monobehaviour_head()
    script = head.m_Script.deref_parse_as_object()
    return script.m_ClassName, reader.read_typetree(
        reader.generate_monobehaviour_node()
    )


def mono_name(reader: Any) -> str:
    head = reader.parse_monobehaviour_head()
    return head.m_Script.deref_parse_as_object().m_ClassName


def deref(owner: Any, reference: dict[str, int]) -> Any:
    if reference["m_PathID"] == 0:
        return None
    return PPtr(
        m_FileID=reference["m_FileID"],
        m_PathID=reference["m_PathID"],
        assetsfile=owner,
    ).deref()


def game_object_transform(game_object: Any) -> Any:
    for pair in game_object.m_Component:
        reader = pair.component.deref()
        if reader.type.name in {"Transform", "RectTransform"}:
            return reader
    raise ValueError(f"game object {game_object.m_Name!r} has no transform")


def quaternion_multiply(
    left: tuple[float, float, float, float],
    right: tuple[float, float, float, float],
) -> tuple[float, float, float, float]:
    lx, ly, lz, lw = left
    rx, ry, rz, rw = right
    return (
        lw * rx + lx * rw + ly * rz - lz * ry,
        lw * ry - lx * rz + ly * rw + lz * rx,
        lw * rz + lx * ry - ly * rx + lz * rw,
        lw * rw - lx * rx - ly * ry - lz * rz,
    )


def quaternion_rotate(
    quaternion: tuple[float, float, float, float],
    vector: tuple[float, float, float],
) -> tuple[float, float, float]:
    vector_quaternion = (vector[0], vector[1], vector[2], 0.0)
    inverse = (-quaternion[0], -quaternion[1], -quaternion[2], quaternion[3])
    rotated = quaternion_multiply(
        quaternion_multiply(quaternion, vector_quaternion), inverse
    )
    return rotated[:3]


def vector(source: Any) -> tuple[float, float, float]:
    return (float(source.x), float(source.y), float(source.z))


class Transforms:
    def __init__(self) -> None:
        self.cache: dict[tuple[str, int], tuple[Any, ...]] = {}

    def world(
        self, reader: Any
    ) -> tuple[
        tuple[float, float, float],
        tuple[float, float, float, float],
        tuple[float, float, float],
    ]:
        key = (str(reader.assets_file.name), reader.path_id)
        if key in self.cache:
            return self.cache[key]  # type: ignore[return-value]
        transform = reader.read()
        position = vector(transform.m_LocalPosition)
        rotation = (
            float(transform.m_LocalRotation.x),
            float(transform.m_LocalRotation.y),
            float(transform.m_LocalRotation.z),
            float(transform.m_LocalRotation.w),
        )
        scale = vector(transform.m_LocalScale)
        if transform.m_Father.path_id:
            parent_position, parent_rotation, parent_scale = self.world(
                transform.m_Father.deref()
            )
            scaled = tuple(position[index] * parent_scale[index] for index in range(3))
            offset = quaternion_rotate(parent_rotation, scaled)
            position = tuple(
                parent_position[index] + offset[index] for index in range(3)
            )
            rotation = quaternion_multiply(parent_rotation, rotation)
            scale = tuple(scale[index] * parent_scale[index] for index in range(3))
        result = (position, rotation, scale)
        self.cache[key] = result
        return result

    def inverse_point(
        self, reader: Any, point: tuple[float, float, float]
    ) -> tuple[float, float, float]:
        position, rotation, scale = self.world(reader)
        offset = tuple(point[index] - position[index] for index in range(3))
        local = quaternion_rotate(
            (-rotation[0], -rotation[1], -rotation[2], rotation[3]), offset
        )
        return tuple(
            local[index] / scale[index] if scale[index] else 0.0 for index in range(3)
        )

    def point(
        self, reader: Any, local: tuple[float, float, float]
    ) -> tuple[float, float, float]:
        position, rotation, scale = self.world(reader)
        scaled = tuple(local[index] * scale[index] for index in range(3))
        offset = quaternion_rotate(rotation, scaled)
        return tuple(position[index] + offset[index] for index in range(3))


SYSTEM_CLASSES = {
    "AnimatorOverrides",
    "ConveyorStation",
    "FireballSpawner",
    "MeteorManager",
    "PressureSwitchCosmeticDecisions",
    "RespawnCollider",
    "SwitchCosmeticDecisions",
    "TriggerAdapter",
    "TriggerAnimatorSetVariable",
    "TriggerDisableScript",
    "TriggerOnAnimator",
    "TriggerTimer",
    "TriggerZone",
}

STATION_CLASSES = {
    "AttachStation",
    "CookingStation",
    "PickupItemSpawner",
    "PlateReturnStation",
    "PlateStation",
    "RubbishBin",
    "WashingStation",
    "Workstation",
}
FEATURE_CLASSES = STATION_CLASSES | {"Interactable"}


class SceneExtractor:
    def __init__(self, environment: Any, scenes: dict[str, int]) -> None:
        self.environment = environment
        self.scenes = scenes
        self.transforms = Transforms()
        self.animated_transform_cache: dict[int, set[int]] = {}
        self.barrier_animator_cache: dict[int, bool] = {}

    def component_names(self, game_object: Any) -> dict[str, Any]:
        result = {}
        for pair in game_object.m_Component:
            reader = pair.component.deref()
            if reader.type.name != "MonoBehaviour":
                result[reader.type.name] = None
                continue
            head = reader.parse_monobehaviour_head()
            script = head.m_Script.deref_parse_as_object()
            result[script.m_ClassName] = reader
        return result

    def animator_ancestor(self, transform: Any) -> Any | None:
        current = transform
        while current is not None:
            data = current.read()
            game_object = data.m_GameObject.deref_parse_as_object()
            for pair in game_object.m_Component:
                reader = pair.component.deref()
                if reader.type.name != "Animator":
                    continue
                if reader.read_typetree()["m_Controller"]["m_PathID"]:
                    return reader
            current = data.m_Father.deref() if data.m_Father.path_id else None
        return None

    def motion_id(self, transform: Any) -> str | None:
        animator = self.animator_ancestor(transform)
        return f"animator-{animator.path_id}" if animator is not None else None

    def animator_transform_paths(self, animator: Any) -> dict[int, Any]:
        root = game_object_transform(animator.read().m_GameObject.deref_parse_as_object())
        result = {0: root}

        def visit(transform: Any, parent: str) -> None:
            for child_pointer in transform.read().m_Children:
                child = child_pointer.deref()
                game_object = child.read().m_GameObject.deref_parse_as_object()
                path = f"{parent}/{game_object.m_Name}" if parent else game_object.m_Name
                result[zlib.crc32(path.encode()) & 0xFFFF_FFFF] = child
                visit(child, path)

        visit(root, "")
        return result

    def animated_transforms(self, animator: Any) -> set[int]:
        cached = self.animated_transform_cache.get(animator.path_id)
        if cached is not None:
            return cached
        animator_data = animator.read_typetree()
        controller = deref(animator.assets_file, animator_data["m_Controller"])
        paths = self.animator_transform_paths(animator)
        result = set()
        if controller is not None:
            for reference in controller.read_typetree()["m_AnimationClips"]:
                clip = deref(controller.assets_file, reference)
                if clip is None:
                    continue
                for binding in clip.read_typetree()["m_ClipBindingConstant"][
                    "genericBindings"
                ]:
                    transform = paths.get(binding["path"])
                    if binding["classID"] == 4 and transform is not None:
                        result.add(transform.path_id)
        self.animated_transform_cache[animator.path_id] = result
        return result

    def is_animated_barrier(self, transform: Any) -> bool:
        animator = self.animator_ancestor(transform)
        if animator is None or transform.path_id not in self.animated_transforms(animator):
            return False
        cached = self.barrier_animator_cache.get(animator.path_id)
        if cached is not None:
            return cached
        animator_data = animator.read_typetree()
        controller = deref(animator.assets_file, animator_data["m_Controller"])
        names = (
            set(dict(controller.read_typetree()["m_TOS"]).values())
            if controller is not None
            else set()
        )
        result = "Idle Closed" in names and "Idle Opened" in names
        self.barrier_animator_cache[animator.path_id] = result
        return result

    def prefab_name(self, owner: Any, reference: dict[str, int]) -> str | None:
        reader = deref(owner, reference)
        if reader is None:
            return None
        if reader.type.name == "GameObject":
            return reader.read().m_Name
        if reader.type.name == "MonoBehaviour":
            return (
                reader.parse_monobehaviour_head()
                .m_GameObject.deref_parse_as_object()
                .m_Name
            )
        return reader.type.name

    def prefab_game_object(self, owner: Any, reference: dict[str, int]) -> Any | None:
        reader = deref(owner, reference)
        if reader is None:
            return None
        if reader.type.name == "GameObject":
            return reader.read()
        if reader.type.name == "MonoBehaviour":
            return (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
        raise ValueError(f"prefab reference has unsupported type {reader.type.name}")

    def game_object_tree(self, root: Any) -> list[Any]:
        result = [root]
        pending = [root]
        while pending:
            current = pending.pop()
            transform = game_object_transform(current).read()
            children = [
                child.deref_parse_as_object().m_GameObject.deref_parse_as_object()
                for child in transform.m_Children
            ]
            result.extend(children)
            pending.extend(children)
        return result

    def ingredient_order(self, game_object: Any) -> str | None:
        for candidate in self.game_object_tree(game_object):
            components = self.component_names(candidate)
            for class_name in (
                "IngredientPropertiesComponent",
                "PreparationContainer",
                "CookablePreparationContainer",
            ):
                component = components.get(class_name)
                if component is not None:
                    _, data = mono(component)
                    order = deref(component.assets_file, data["m_ingredientOrderNode"])
                    return mono(order)[1]["m_Name"] if order is not None else None
        return None

    def food_properties(self, game_object: Any) -> dict[str, Any]:
        result: dict[str, Any] = {"order": self.ingredient_order(game_object)}
        for candidate in self.game_object_tree(game_object):
            components = self.component_names(candidate)
            container = components.get("IngredientContainer")
            if container is not None:
                result["container_capacity"] = mono(container)[1]["m_capacity"]
            cooking = components.get("CookingHandler")
            if cooking is not None:
                _, data = mono(cooking)
                step = deref(cooking.assets_file, data["m_cookingType"])
                result["cooking"] = {
                    "seconds": data["m_cookingtime"],
                    "step": mono(step)[1]["m_Name"] if step is not None else None,
                    "station_type": data["m_stationType"],
                }
        return result

    def spawner(self, reader: Any, data: dict[str, Any]) -> dict[str, Any]:
        prefab_reader = deref(reader.assets_file, data["m_itemPrefab"])
        if prefab_reader is None:
            raise ValueError("PickupItemSpawner has no item prefab")
        prefab = prefab_reader.read()
        result = {
            "ingredient": prefab.m_Name,
            "ingredient_properties": self.food_properties(prefab),
            "pickup_priority": data["m_pickupPriority"],
        }
        components = self.component_names(prefab)
        workable = components.get("WorkableItem")
        if workable is not None:
            _, workable_data = mono(workable)
            processed = self.prefab_game_object(
                workable.assets_file, workable_data["m_nextPrefab"]
            )
            result.update(
                {
                    "work_stages": workable_data["m_stages"],
                    "processed_item": processed.m_Name
                    if processed is not None
                    else None,
                    "processed_properties": (
                        self.food_properties(processed) if processed is not None else {}
                    ),
                }
            )
        return result

    def feature(self, reader: Any, class_name: str) -> dict[str, Any]:
        _, data = mono(reader)
        if class_name == "Interactable":
            return {
                "started_trigger": data["m_onInteractStartedTrigger"],
                "ended_trigger": data["m_onInteractEndedTrigger"],
                "impulse_trigger": data["m_onInteractImpulseTrigger"],
                "allow_multiple": bool(data["m_allowMultipleInteracters"]),
                "use_placement_button": bool(data["m_usePlacementButton"]),
            }
        if class_name == "PickupItemSpawner":
            return self.spawner(reader, data)
        if class_name == "PlateReturnStation":
            return {
                "startingPlateNumber": data["m_startingPlateNumber"],
                "stackPrefab": self.prefab_name(
                    reader.assets_file, data["m_stackPrefab"]
                ),
            }
        if class_name == "PlateStation":
            return {
                "createPlateTime": data["m_createPlateTime"],
                "teamId": data["m_teamId"],
                "returnStation": self.component_object_id(
                    reader.assets_file, data["m_returnStation"]
                ),
            }
        if class_name == "WashingStation":
            return {
                "cleanPlateTime": data["m_cleanPlateTime"],
                "dryingStation": self.component_object_id(
                    reader.assets_file, data["m_dryingStation"]
                ),
                "dirtyPlateSlots": len(data["m_dirtyPlates"]),
            }
        fields = {
            "AttachStation": ("m_pickupPriority", "m_placementPriority"),
            "CookingStation": ("m_stationType", "m_attachRestrictions"),
            "RubbishBin": ("m_fallTime",),
            "Workstation": ("m_chopTrigger",),
        }[class_name]
        return {field.removeprefix("m_"): data[field] for field in fields}

    def component_object_id(self, owner: Any, reference: dict[str, int]) -> str | None:
        component = deref(owner, reference)
        if component is None:
            return None
        if component.type.name != "MonoBehaviour":
            raise ValueError(f"expected component reference, got {component.type.name}")
        game_object = (
            component.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
        )
        return f"object-{game_object_transform(game_object).path_id}"

    def game_object_record(
        self,
        game_object: Any,
        components: dict[str, Any],
        manager: dict[str, Any],
        dynamic: bool,
    ) -> dict[str, Any]:
        transform = game_object_transform(game_object)
        position = self.transforms.world(transform)[0]
        grid = self.grid_index(manager, position)
        features = {
            name: self.feature(reader, name)
            for name, reader in components.items()
            if name in FEATURE_CLASSES
        }
        result = {
            "id": f"object-{game_object_transform(game_object).path_id}",
            "name": game_object.m_Name,
            "layer": game_object.m_Layer,
            "grid_manager": manager["id"],
            "grid": {"x": grid[0], "y": grid[1], "z": grid[2]},
            "world": {
                "x": round(position[0], 5),
                "y": round(position[1], 5),
                "z": round(position[2], 5),
            },
            "dynamic": dynamic,
            "components": sorted(components),
            "features": features,
        }
        if dynamic:
            result["motion"] = self.motion_id(transform)
            local = transform.read()
            result["motion_transform"] = f"transform-{transform.path_id}"
            result["motion_local_position"] = {
                "x": local.m_LocalPosition.x,
                "y": local.m_LocalPosition.y,
                "z": local.m_LocalPosition.z,
            }
        return result

    def grid_managers(self, scene: Any) -> list[dict[str, Any]]:
        managers = []
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            if mono_name(reader) != "QuadGridManager":
                continue
            _, data = mono(reader)
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            managers.append(
                {
                    "id": f"grid-{transform.path_id}",
                    "name": game_object.m_Name,
                    "transform": transform,
                    "half_size": data["m_gridHalfSize"],
                    "origin": data["m_origin"],
                    "size": data["m_size"],
                    "world": dict(
                        zip(
                            ("x", "y", "z"),
                            (
                                round(value, 5)
                                for value in self.transforms.world(transform)[0]
                            ),
                        )
                    ),
                    "motion": self.motion_id(transform),
                }
            )
        if not managers:
            raise ValueError(f"{scene.name}: no QuadGridManager")
        return managers

    def manager_for(
        self,
        transform_reader: Any,
        managers: list[dict[str, Any]],
        world: tuple[float, float, float] | None = None,
    ) -> dict[str, Any]:
        by_transform = {row["transform"].path_id: row for row in managers}
        ancestry = []
        current = transform_reader
        while current is not None:
            if current.path_id in by_transform:
                ancestry.append(by_transform[current.path_id])
            parent = current.read().m_Father
            current = parent.deref() if parent.path_id else None
        point = world or self.transforms.world(transform_reader)[0]
        valid = []
        for manager in managers:
            index = self.grid_index(manager, point)
            half = manager["half_size"]
            if all(
                abs(index[position]) <= half[axis]
                for position, axis in enumerate(("X", "Y", "Z"))
            ):
                valid.append(manager)
        for manager in ancestry:
            if manager in valid:
                return manager
        if valid:
            return min(
                valid,
                key=lambda manager: (
                    manager["half_size"]["X"]
                    * manager["half_size"]["Y"]
                    * manager["half_size"]["Z"]
                ),
            )
        if ancestry:
            return ancestry[0]
        roots = [
            manager
            for manager in managers
            if not manager["transform"].read().m_Father.path_id
            or manager["name"] == "GridManager"
        ]
        return roots[0] if roots else managers[0]

    def grid_index(
        self, manager: dict[str, Any], world: tuple[float, float, float]
    ) -> tuple[int, int, int]:
        local = self.transforms.inverse_point(manager["transform"], world)
        origin = manager["origin"]
        size = manager["size"]
        return tuple(
            round(
                (local[index] - origin[("x", "y", "z")[index]])
                / size[("x", "y", "z")[index]]
            )
            for index in range(3)
        )  # type: ignore[return-value]

    def grid_point(
        self, manager: dict[str, Any], index: tuple[int, int, int]
    ) -> tuple[float, float, float]:
        origin = manager["origin"]
        size = manager["size"]
        local = tuple(
            origin[axis] + index[position] * size[axis]
            for position, axis in enumerate(("x", "y", "z"))
        )
        return self.transforms.point(manager["transform"], local)

    def colliders(
        self, scene: Any, managers: list[dict[str, Any]], layers: set[int]
    ) -> list[dict[str, Any]]:
        colliders = []
        for reader in scene.objects.values():
            if reader.type.name not in {"BoxCollider", "MeshCollider"}:
                continue
            collider = reader.read()
            game_object = collider.m_GameObject.deref_parse_as_object()
            if game_object.m_Layer not in layers or not collider.m_Enabled:
                continue
            transform = game_object_transform(game_object)
            if reader.type.name == "BoxCollider":
                center = vector(collider.m_Center)
                size = vector(collider.m_Size)
                extent = tuple(value / 2 for value in size)
            else:
                mesh = collider.m_Mesh.deref().read_typetree()
                bounds = mesh["m_LocalAABB"]
                center = tuple(bounds["m_Center"][axis] for axis in ("x", "y", "z"))
                extent = tuple(bounds["m_Extent"][axis] for axis in ("x", "y", "z"))
            world_center = self.transforms.point(transform, center)
            surface_y = max(
                self.transforms.point(
                    transform,
                    tuple(
                        center[index] + extent[index] * sign[index]
                        for index in range(3)
                    ),
                )[1]
                for sign in (
                    (-1, -1, -1),
                    (-1, -1, 1),
                    (-1, 1, -1),
                    (-1, 1, 1),
                    (1, -1, -1),
                    (1, -1, 1),
                    (1, 1, -1),
                    (1, 1, 1),
                )
            )
            colliders.append(
                {
                    "transform": transform,
                    "motion": self.motion_id(transform),
                    "manager": self.manager_for(transform, managers, world_center)[
                        "id"
                    ],
                    "center": center,
                    "extent": extent,
                    "surface_y": surface_y,
                }
            )
        return colliders

    def contains(
        self,
        collider: dict[str, Any],
        point: tuple[float, float, float],
        padding: float = 0.03,
    ) -> bool:
        local = self.transforms.inverse_point(collider["transform"], point)
        return all(
            abs(local[index] - collider["center"][index])
            <= collider["extent"][index]
            + (0.18 if collider["extent"][index] < 0.01 else padding)
            for index in range(3)
        )

    def supports(
        self,
        collider: dict[str, Any],
        point: tuple[float, float, float],
    ) -> bool:
        if self.contains(collider, point):
            return True
        surface = (point[0], collider["surface_y"] - 0.01, point[2])
        return (
            abs(point[1] - collider["surface_y"]) < 0.15
            and self.contains(collider, surface)
        )

    def walkable(
        self,
        managers: list[dict[str, Any]],
        ground: list[dict[str, Any]],
        walls: list[dict[str, Any]],
        occupied: set[tuple[str, int, int, int]],
        players: list[dict[str, Any]],
        grid_objects: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        cells: list[dict[str, Any]] = []
        for manager in managers:
            half = manager["half_size"]
            anchors = [
                (row["grid"]["x"], row["grid"]["y"], row["grid"]["z"])
                for row in [*players, *grid_objects]
                if row["grid_manager"] == manager["id"]
            ]
            if anchors:
                min_x = min(row[0] for row in anchors) - 1
                max_x = max(row[0] for row in anchors) + 1
                min_z = min(row[2] for row in anchors) - 1
                max_z = max(row[2] for row in anchors) + 1
            else:
                min_x, max_x = -half["X"], half["X"]
                min_z, max_z = -half["Z"], half["Z"]
            relevant_ground = [
                collider for collider in ground if collider["manager"] == manager["id"]
            ]
            fixed_ground = [
                collider
                for collider in relevant_ground
                if collider["motion"] in {None, manager["motion"]}
            ]
            moving_ground = [
                collider
                for collider in relevant_ground
                if collider["motion"] not in {None, manager["motion"]}
            ]
            relevant_walls = [
                collider
                for collider in walls
                if collider["manager"] == manager["id"] and collider["motion"] is None
            ]
            candidates: set[tuple[int, int, int]] = set()
            for x in range(max(-half["X"], min_x), min(half["X"], max_x) + 1):
                for z in range(max(-half["Z"], min_z), min(half["Z"], max_z) + 1):
                    for y in range(-half["Y"], half["Y"] + 1):
                        key = (manager["id"], x, y, z)
                        if key in occupied:
                            continue
                        point = self.grid_point(manager, (x, y, z))
                        if any(
                            self.supports(collider, point)
                            for collider in fixed_ground
                        ) and not any(
                            self.contains(collider, point, padding=0.08)
                            for collider in relevant_walls
                        ):
                            candidates.add((x, y, z))
            seeds = {
                (row["grid"]["x"], row["grid"]["y"], row["grid"]["z"])
                for row in players
                if row["grid_manager"] == manager["id"]
            }
            for row in grid_objects:
                if row["grid_manager"] != manager["id"] or not row["features"]:
                    continue
                x, y, z = (
                    row["grid"]["x"],
                    row["grid"]["y"],
                    row["grid"]["z"],
                )
                seeds.update(
                    (x + dx, y, z + dz) for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1))
                )
            # Irregular floor pieces do not always cover the exact grid-cell
            # centre even though the authored player transform is supported.
            # Preserve those authored start cells as traversal seeds.
            candidates.update(
                (row["grid"]["x"], row["grid"]["y"], row["grid"]["z"])
                for row in players
                if row["grid_manager"] == manager["id"]
            )
            seeds &= candidates
            if not seeds and candidates:
                seeds.add(min(candidates))
            reachable = set(seeds)
            pending = list(seeds)
            while pending:
                current = pending.pop()
                current_world = self.grid_point(manager, current)
                for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    neighbor = (
                        current[0] + dx,
                        current[1],
                        current[2] + dz,
                    )
                    if neighbor not in candidates or neighbor in reachable:
                        continue
                    neighbor_world = self.grid_point(manager, neighbor)
                    midpoint = tuple(
                        (current_world[index] + neighbor_world[index]) / 2
                        for index in range(3)
                    )
                    if any(
                        self.contains(collider, midpoint, padding=0.08)
                        for collider in relevant_walls
                    ):
                        continue
                    reachable.add(neighbor)
                    pending.append(neighbor)
            cells.extend(
                {
                    "grid_manager": manager["id"],
                    "x": x,
                    "y": y,
                    "z": z,
                    "world": dict(
                        zip(
                            ("x", "y", "z"),
                            (
                                round(value, 5)
                                for value in self.grid_point(manager, (x, y, z))
                            ),
                        )
                    ),
                }
                for x, y, z in sorted(reachable)
            )
            moving_cells = set()
            for x in range(-half["X"], half["X"] + 1):
                for z in range(-half["Z"], half["Z"] + 1):
                    column = self.grid_point(manager, (x, 0, z))
                    for collider in moving_ground:
                        y = self.grid_index(
                            manager,
                            (column[0], collider["surface_y"], column[2]),
                        )[1]
                        point = self.grid_point(manager, (x, y, z))
                        if not self.supports(collider, point):
                            continue
                        moving_cells.add((collider["motion"], x, y, z))
            cells.extend(
                {
                    "grid_manager": manager["id"],
                    "x": x,
                    "y": y,
                    "z": z,
                    "world": dict(
                        zip(
                            ("x", "y", "z"),
                            (
                                round(value, 5)
                                for value in self.grid_point(manager, (x, y, z))
                            ),
                        )
                    ),
                    "motion": motion,
                }
                for motion, x, y, z in sorted(moving_cells)
            )
        return cells

    def fall_edges(
        self,
        cells: list[dict[str, Any]],
        managers: list[dict[str, Any]],
        ground: list[dict[str, Any]],
        walls: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        fixed_cells = [cell for cell in cells if cell.get("motion") is None]
        cell_keys = {
            (cell["grid_manager"], cell["x"], cell["y"], cell["z"])
            for cell in fixed_cells
        }
        world_cells = [
            tuple(cell["world"][axis] for axis in ("x", "y", "z"))
            for cell in fixed_cells
        ]
        manager_by_id = {manager["id"]: manager for manager in managers}
        result = []
        for cell in fixed_cells:
            manager = manager_by_id[cell["grid_manager"]]
            fixed_ground = [
                collider
                for collider in ground
                if collider["motion"] in {None, manager["motion"]}
            ]
            static_walls = [wall for wall in walls if wall["motion"] is None]
            current = tuple(cell["world"][axis] for axis in ("x", "y", "z"))
            for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                target_grid = (
                    cell["x"] + dx,
                    cell["y"],
                    cell["z"] + dz,
                )
                if (manager["id"], *target_grid) in cell_keys:
                    continue
                target = self.grid_point(manager, target_grid)
                if any(
                    math.hypot(other[0] - target[0], other[2] - target[2]) < 0.45
                    and abs(other[1] - target[1]) < 0.65
                    for other in world_cells
                ):
                    continue
                if any(self.supports(collider, target) for collider in fixed_ground):
                    continue
                midpoint = tuple(
                    (current[index] + target[index]) / 2 for index in range(3)
                )
                if any(
                    self.contains(collider, midpoint, padding=0.08)
                    for collider in static_walls
                ):
                    continue
                result.append(
                    {
                        "grid_manager": manager["id"],
                        "from": {
                            "x": cell["x"],
                            "y": cell["y"],
                            "z": cell["z"],
                        },
                        "dx": dx,
                        "dz": dz,
                    }
                )
        return result

    def snap_players_to_ground(
        self,
        players: list[dict[str, Any]],
        managers: list[dict[str, Any]],
        ground: list[dict[str, Any]],
    ) -> None:
        by_id = {manager["id"]: manager for manager in managers}
        for player in players:
            point = tuple(player["world"][axis] for axis in ("x", "y", "z"))
            supported = [
                collider
                for collider in ground
                if self.contains(collider, point, padding=0.1)
            ]
            if not supported:
                continue
            current = player["grid_manager"]
            manager_id = (
                current
                if any(row["manager"] == current for row in supported)
                else supported[0]["manager"]
            )
            manager = by_id[manager_id]
            grid = self.grid_index(manager, point)
            player["grid_manager"] = manager_id
            player["grid"] = dict(zip(("x", "y", "z"), grid))

    def cooking_utensils(
        self,
        scene: Any,
        managers: list[dict[str, Any]],
        grid_objects: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        utensils = []
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            if mono_name(reader) != "CookingHandler":
                continue
            _, data = mono(reader)
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            world = self.transforms.world(transform)[0]
            if not any(
                "AttachStation" in object["components"]
                and sum(
                    (object["world"][axis] - world[index]) ** 2
                    for index, axis in enumerate(("x", "y", "z"))
                )
                <= 1.0
                for object in grid_objects
            ):
                continue
            manager = self.manager_for(transform, managers, world)
            grid = self.grid_index(manager, world)
            cooking_step = deref(reader.assets_file, data["m_cookingType"])
            components = self.component_names(game_object)
            container = components.get("IngredientContainer")
            container_capacity = (
                mono(container)[1]["m_capacity"] if container is not None else 0
            )
            utensils.append(
                {
                    "id": f"utensil-{transform.path_id}",
                    "name": game_object.m_Name,
                    "grid_manager": manager["id"],
                    "grid": {"x": grid[0], "y": grid[1], "z": grid[2]},
                    "world": dict(
                        zip(("x", "y", "z"), (round(value, 5) for value in world))
                    ),
                    "cooking_seconds": data["m_cookingtime"],
                    "cooking_step": (
                        mono(cooking_step)[1]["m_Name"]
                        if cooking_step is not None
                        else None
                    ),
                    "container_capacity": container_capacity,
                    "station_type": data["m_stationType"],
                }
            )
        return utensils

    def plates(
        self,
        scene: Any,
        managers: list[dict[str, Any]],
        grid_objects: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        plates = []
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour" or mono_name(reader) != "Plate":
                continue
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            world = self.transforms.world(transform)[0]
            if not any(
                "AttachStation" in object["components"]
                and sum(
                    (object["world"][axis] - world[index]) ** 2
                    for index, axis in enumerate(("x", "y", "z"))
                )
                <= 1.0
                for object in grid_objects
            ):
                continue
            manager = self.manager_for(transform, managers, world)
            grid = self.grid_index(manager, world)
            plates.append(
                {
                    "id": f"plate-{transform.path_id}",
                    "name": game_object.m_Name,
                    "grid_manager": manager["id"],
                    "grid": {"x": grid[0], "y": grid[1], "z": grid[2]},
                    "world": dict(
                        zip(("x", "y", "z"), (round(value, 5) for value in world))
                    ),
                }
            )
        return plates

    def fire_extinguishers(
        self, scene: Any, managers: list[dict[str, Any]]
    ) -> list[dict[str, Any]]:
        extinguishers = []
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            if mono_name(reader) != "FireExtinguishSpray":
                continue
            _, data = mono(reader)
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            world = self.transforms.world(transform)[0]
            manager = self.manager_for(transform, managers, world)
            grid = self.grid_index(manager, world)
            extinguishers.append(
                {
                    "id": f"extinguisher-{transform.path_id}",
                    "name": game_object.m_Name,
                    "grid_manager": manager["id"],
                    "grid": {"x": grid[0], "y": grid[1], "z": grid[2]},
                    "world": dict(
                        zip(
                            ("x", "y", "z"),
                            (round(value, 5) for value in world),
                        )
                    ),
                    "extinguish_seconds": data["m_exinguishTime"],
                    "spray_distance": data["m_sprayDistance"],
                }
            )
        return extinguishers

    def order_capacity(self, scene: Any) -> int:
        for reader in scene.objects.values():
            if (
                reader.type.name == "MonoBehaviour"
                and mono_name(reader) == "RecipeFlowGUI"
            ):
                return int(mono(reader)[1]["m_maxOrdersAllowed"])
        raise ValueError(f"{scene.name}: no RecipeFlowGUI")

    def boss_flow(self, scene: Any) -> dict[str, Any] | None:
        for reader in scene.objects.values():
            if (
                reader.type.name != "MonoBehaviour"
                or mono_name(reader) != "BossFlowController"
            ):
                continue
            _, data = mono(reader)
            return {
                "platforms": [
                    f"animator-{deref(reader.assets_file, reference).path_id}"
                    for reference in data["m_platforms"]
                    if deref(reader.assets_file, reference) is not None
                ],
                # BossFlowController.BuildRunLevelRoutine uses a literal
                # five-second round intermission before raising the old
                # platform and lowering the next one.
                "intermission_seconds": 5.0,
            }
        return None

    def players(
        self, scene: Any, managers: list[dict[str, Any]]
    ) -> list[dict[str, Any]]:
        players = []
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            if mono_name(reader) != "PlayerControls":
                continue
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            world = self.transforms.world(transform)[0]
            manager = self.manager_for(transform, managers, world)
            grid = self.grid_index(manager, world)
            components = self.component_names(game_object)
            identity = components.get("PlayerIDProvider")
            identity_data = mono(identity)[1] if identity is not None else {}
            respawn = components.get("PlayerRespawnBehaviour")
            respawn_data = mono(respawn)[1] if respawn is not None else {}
            players.append(
                {
                    "id": identity_data.get("m_player", game_object.m_Name),
                    "name": game_object.m_Name,
                    "grid_manager": manager["id"],
                    "grid": {"x": grid[0], "y": grid[1], "z": grid[2]},
                    "world": dict(
                        zip(("x", "y", "z"), (round(value, 5) for value in world))
                    ),
                    "respawn_seconds": respawn_data.get("m_respawnTime", 5.0),
                    "spawn_effect_seconds": respawn_data.get("m_particleTime", 1.0),
                }
            )
        return sorted(players, key=lambda row: str(row["id"]))

    def systems(
        self,
        scene: Any,
        managers: list[dict[str, Any]],
        grid_objects: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        round_enabled: dict[int, bool] = {}
        for activation in scene.objects.values():
            if (
                activation.type.name != "MonoBehaviour"
                or mono_name(activation) != "FlowbasedComponentActivation"
            ):
                continue
            _, activation_data = mono(activation)
            target = deref(activation.assets_file, activation_data["m_targetComponent"])
            if target is not None:
                round_enabled[target.path_id] = bool(
                    activation_data.get("m_activeInRound", 1)
                )
        systems = []
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            class_name = mono_name(reader)
            if class_name not in SYSTEM_CLASSES:
                continue
            _, data = mono(reader)
            head = reader.parse_monobehaviour_head()
            game_object = head.m_GameObject.deref_parse_as_object()
            transform = game_object_transform(game_object)
            world = self.transforms.world(transform)[0]
            manager = self.manager_for(transform, managers, world)
            grid = self.grid_index(manager, world)
            record = {
                "id": f"system-{reader.path_id}",
                "kind": class_name,
                "name": game_object.m_Name,
                "object": f"object-{transform.path_id}",
                "enabled": round_enabled.get(
                    reader.path_id, bool(data.get("m_Enabled", 1))
                ),
                "grid_manager": manager["id"],
                "grid": dict(zip(("x", "y", "z"), grid)),
                "world": dict(
                    zip(("x", "y", "z"), (round(value, 5) for value in world))
                ),
                "fields": {
                    key: value
                    for key, value in data.items()
                    if key not in {"m_GameObject", "m_Enabled", "m_Script", "m_Name"}
                },
            }
            target_animator = data.get("m_targetAnimator")
            if target_animator and target_animator["m_PathID"]:
                record["target_animator"] = f"animator-{target_animator['m_PathID']}"
            motion = self.motion_id(transform)
            if motion is not None and class_name == "FireballSpawner":
                record["motion"] = motion
            if class_name == "FireballSpawner":
                target = deref(reader.assets_file, data["m_target"])
                if target is not None:
                    target_world = self.transforms.world(target)[0]
                    record["target_world"] = dict(
                        zip(
                            ("x", "y", "z"),
                            (round(value, 5) for value in target_world),
                        )
                    )
            if class_name == "MeteorManager":
                minimum = data["m_minGridIndex"]
                maximum = data["m_maxGridIndex"]
                record["targets"] = [
                    {
                        "grid": {"x": x, "y": y, "z": z},
                        "world": dict(
                            zip(
                                ("x", "y", "z"),
                                (
                                    round(value, 5)
                                    for value in self.grid_point(manager, (x, y, z))
                                ),
                            )
                        ),
                    }
                    for x in range(minimum["X"], maximum["X"] + 1)
                    for y in range(minimum["Y"], maximum["Y"] + 1)
                    for z in range(minimum["Z"], maximum["Z"] + 1)
                ]
            if class_name == "ConveyorStation":
                rotation = self.transforms.world(transform)[1]
                right = quaternion_rotate(rotation, (1.0, 0.0, 0.0))
                offset_x = round(-right[0])
                offset_z = round(-right[2])
                if data["m_conveyanceDirectionXZ"] == 0:
                    offset_x = -offset_x
                    offset_z = -offset_z
                target_grid = (grid[0] + offset_x, grid[1], grid[2] + offset_z)
                record["target_grid"] = dict(zip(("x", "y", "z"), target_grid))
                receivers = [
                    row
                    for row in grid_objects
                    if row["grid_manager"] == manager["id"]
                    and (
                        row["grid"]["x"],
                        row["grid"]["y"],
                        row["grid"]["z"],
                    )
                    == target_grid
                    and "TabletopConveyenceReceiver" in row["components"]
                ]
                if len(receivers) > 1:
                    raise ValueError(
                        f"{game_object.m_Name}: ambiguous conveyor destination"
                    )
                if receivers:
                    record["target_object"] = receivers[0]["id"]
            systems.append(record)
        return systems

    @staticmethod
    def streamed_frames(words: list[int]) -> dict[int, list[dict[str, Any]]]:
        curves: dict[int, list[dict[str, Any]]] = {}
        cursor = 0
        while cursor < len(words):
            time = struct.unpack("<f", struct.pack("<I", words[cursor]))[0]
            count = words[cursor + 1]
            cursor += 2
            for _ in range(count):
                index = words[cursor]
                coefficients = [
                    struct.unpack("<f", struct.pack("<I", value))[0]
                    for value in words[cursor + 1 : cursor + 5]
                ]
                cursor += 5
                if math.isfinite(time):
                    curves.setdefault(index, []).append(
                        {
                            "time": round(time, 6),
                            "coefficients": [round(value, 7) for value in coefficients],
                        }
                    )
        return curves

    @staticmethod
    def binding_width(binding: dict[str, Any]) -> int:
        if binding["classID"] != 4:
            return 1
        return {1: 3, 2: 4, 3: 3, 4: 3}.get(binding["attribute"], 1)

    def animation_clip(
        self,
        reader: Any,
        transform_paths: dict[int, Any],
        tracked_transforms: set[str],
        include_properties: bool,
    ) -> dict[str, Any]:
        data = reader.read_typetree()
        clip = data["m_MuscleClip"]["m_Clip"]["data"]
        streamed = clip["m_StreamedClip"]
        dense = clip["m_DenseClip"]
        streamed_curves = self.streamed_frames(streamed["data"])
        stream_count = streamed["curveCount"]
        dense_count = dense["m_CurveCount"]
        constants = (clip.get("m_ConstantClip") or {}).get("data", [])
        channels: dict[str, Any] = {}
        transform_channels: dict[str, dict[str, Any]] = {}

        def scalar(index: int) -> dict[str, Any] | None:
            if index < stream_count:
                keys = streamed_curves.get(index, [])
                return {"kind": "curve", "keys": keys} if keys else None
            if index < stream_count + dense_count:
                dense_index = index - stream_count
                return {
                    "kind": "dense",
                    "begin": round(dense["m_BeginTime"], 6),
                    "sample_rate": round(dense["m_SampleRate"], 6),
                    "samples": [
                        round(
                            dense["m_SampleArray"][frame * dense_count + dense_index],
                            7,
                        )
                        for frame in range(dense["m_FrameCount"])
                    ],
                }
            constant_index = index - stream_count - dense_count
            if constant_index < len(constants):
                return {
                    "kind": "constant",
                    "value": round(constants[constant_index], 7),
                }
            return None

        cursor = 0
        properties = {}
        for binding in data["m_ClipBindingConstant"]["genericBindings"]:
            width = self.binding_width(binding)
            if binding["classID"] == 4:
                name = {
                    1: "position",
                    2: "rotation",
                    3: "scale",
                    4: "euler",
                }.get(binding["attribute"])
                if name:
                    target = transform_paths.get(binding["path"])
                    target_id = (
                        f"transform-{target.path_id}" if target is not None else None
                    )
                    target_channels = (
                        channels
                        if binding["path"] == 0 and target_id in tracked_transforms
                        else transform_channels.setdefault(
                            target_id, {}
                        )
                        if target_id in tracked_transforms
                        else None
                    )
                    if target_channels is not None:
                        target_channels[name] = [
                            scalar(cursor + offset) for offset in range(width)
                        ]
            elif (
                include_properties
                and binding["classID"] == 114
                and binding["path"] == 0
            ):
                properties[str(binding["attribute"])] = scalar(cursor)
            cursor += width
        muscle = data["m_MuscleClip"]
        return {
            "id": f"clip-{reader.path_id}",
            "name": data["m_Name"],
            "duration_seconds": round(muscle["m_StopTime"] - muscle["m_StartTime"], 6),
            "loop": bool(muscle["m_LoopTime"]),
            "channels": channels,
            "transform_channels": transform_channels,
            "properties": properties,
        }

    def state_behaviors(
        self, controller: Any, data: dict[str, Any]
    ) -> dict[int, list[dict[str, Any]]]:
        description = data["m_StateMachineBehaviourVectorDescription"]
        indices = description["m_StateMachineBehaviourIndices"]
        references = data["m_StateMachineBehaviours"]
        result: dict[int, list[dict[str, Any]]] = {}
        for key, value in description["m_StateMachineBehaviourRanges"]:
            behaviors = []
            start = value["m_StartIndex"]
            for offset in range(start, start + value["m_Count"]):
                reader = deref(controller.assets_file, references[indices[offset]])
                if reader is None:
                    continue
                class_name, behavior = mono(reader)
                behaviors.append(
                    {
                        "kind": class_name,
                        "fields": {
                            field: field_value
                            for field, field_value in behavior.items()
                            if field
                            not in {
                                "m_GameObject",
                                "m_Enabled",
                                "m_Script",
                                "m_Name",
                            }
                        },
                    }
                )
            result[key["m_StateID"]] = behaviors
        return result

    def animator_motion(
        self,
        animator: Any,
        tracked_transforms: set[str],
        include_properties: bool,
    ) -> dict[str, Any]:
        animator_data = animator.read_typetree()
        game_object = animator.read().m_GameObject.deref_parse_as_object()
        transform_paths = self.animator_transform_paths(animator)
        if tracked_transforms:
            tracked_transforms = {
                *tracked_transforms,
                f"transform-{transform_paths[0].path_id}",
            }
        overrides = {}
        for component in self.component_names(game_object).values():
            if component is None or mono_name(component) != "AnimatorOverrides":
                continue
            _, data = mono(component)
            for pair in data["m_overrides"]:
                original = deref(component.assets_file, pair["originalClip"])
                replacement = deref(component.assets_file, pair["overrideClip"])
                if original is not None and replacement is not None:
                    overrides[(str(original.assets_file.name), original.path_id)] = (
                        replacement
                    )
        controller = deref(animator.assets_file, animator_data["m_Controller"])
        if controller is None:
            raise ValueError("gameplay animator has no controller")
        controller_data = controller.read_typetree()
        tos = dict(controller_data["m_TOS"])
        clip_ids = []
        clips = []
        for reference in controller_data["m_AnimationClips"]:
            reader = deref(controller.assets_file, reference)
            if reader is None:
                clip_ids.append(None)
                continue
            reader = overrides.get(
                (str(reader.assets_file.name), reader.path_id), reader
            )
            clip_id = f"clip-{reader.path_id}"
            clip_ids.append(clip_id)
            if not any(clip["id"] == clip_id for clip in clips):
                clips.append(
                    self.animation_clip(
                        reader,
                        transform_paths,
                        tracked_transforms,
                        include_properties,
                    )
                )
        behaviors = self.state_behaviors(controller, controller_data)
        runtime = controller_data["m_Controller"]
        machine = runtime["m_StateMachineArray"][0]["data"]
        states = []
        for index, wrapped in enumerate(machine["m_StateConstantArray"]):
            state = wrapped["data"]
            blend_trees = state["m_BlendTreeConstantArray"]
            nodes = blend_trees[0]["data"]["m_NodeArray"] if blend_trees else []
            clip_index = nodes[0]["data"]["m_ClipID"] if nodes else None
            states.append(
                {
                    "index": index,
                    "id": state["m_FullPathID"],
                    "name": tos.get(state["m_NameID"], str(state["m_NameID"])),
                    "clip": (
                        clip_ids[clip_index]
                        if clip_index is not None and clip_index < len(clip_ids)
                        else None
                    ),
                    "speed": state["m_Speed"],
                    "cycle_offset": state["m_CycleOffset"],
                    "loop": bool(state["m_Loop"]),
                    "transitions": [
                        {
                            "destination": transition["data"]["m_DestinationState"],
                            "duration": transition["data"]["m_TransitionDuration"],
                            "exit_time": transition["data"]["m_ExitTime"],
                            "has_exit_time": bool(transition["data"]["m_HasExitTime"]),
                            "conditions": [
                                {
                                    "mode": condition["data"]["m_ConditionMode"],
                                    "parameter": tos.get(
                                        condition["data"]["m_EventID"],
                                        str(condition["data"]["m_EventID"]),
                                    ),
                                    "threshold": condition["data"]["m_EventThreshold"],
                                }
                                for condition in transition["data"][
                                    "m_ConditionConstantArray"
                                ]
                            ],
                        }
                        for transition in state["m_TransitionConstantArray"]
                    ],
                    "behaviors": behaviors.get(state["m_FullPathID"], []),
                }
            )
        transform = game_object_transform(game_object)
        local = transform.read()
        world_position, world_rotation, _ = self.transforms.world(transform)
        return {
            "id": f"animator-{animator.path_id}",
            "name": game_object.m_Name,
            "controller": controller_data["m_Name"],
            "transform": f"transform-{transform.path_id}",
            "initial_local_position": dict(
                zip(("x", "y", "z"), vector(local.m_LocalPosition))
            ),
            "initial_local_rotation": {
                "x": local.m_LocalRotation.x,
                "y": local.m_LocalRotation.y,
                "z": local.m_LocalRotation.z,
                "w": local.m_LocalRotation.w,
            },
            "initial_world_position": dict(zip(("x", "y", "z"), world_position)),
            "initial_world_rotation": dict(zip(("x", "y", "z", "w"), world_rotation)),
            "default_state": machine["m_DefaultState"],
            "parameters": [
                {
                    "id": parameter["m_ID"],
                    "name": tos.get(parameter["m_ID"], str(parameter["m_ID"])),
                    "type": parameter["m_Type"],
                }
                for parameter in runtime["m_Values"]["data"]["m_ValueArray"]
            ],
            "default_values": runtime["m_DefaultValues"]["data"],
            "states": states,
            "clips": clips,
        }

    def motions(
        self,
        managers: list[dict[str, Any]],
        grid_objects: list[dict[str, Any]],
        systems: list[dict[str, Any]],
        ground: list[dict[str, Any]],
        scene: Any,
    ) -> list[dict[str, Any]]:
        tracked_transforms: dict[str, set[str]] = {}
        for manager in managers:
            if motion := manager.get("motion"):
                tracked_transforms.setdefault(motion, set()).add(
                    f"transform-{manager['transform'].path_id}"
                )
        for grid_object in grid_objects:
            if motion := grid_object.get("motion"):
                tracked_transforms.setdefault(motion, set()).add(
                    grid_object["motion_transform"]
                )
        manager_motion = {manager["id"]: manager["motion"] for manager in managers}
        for collider in ground:
            motion = collider["motion"]
            if motion is not None and motion != manager_motion[collider["manager"]]:
                tracked_transforms.setdefault(motion, set()).add(
                    f"transform-{collider['transform'].path_id}"
                )
        property_motions = {
            system["motion"]
            for system in systems
            if system.get("motion") is not None
        }
        motion_ids = {
            motion
            for motion in [
                *(manager.get("motion") for manager in managers),
                *(grid_object.get("motion") for grid_object in grid_objects),
                *(system.get("target_animator") for system in systems),
                *(system.get("motion") for system in systems),
                *tracked_transforms,
            ]
            if motion is not None
        }
        all_readers = {
            f"animator-{reader.path_id}": reader
            for reader in scene.objects.values()
            if reader.type.name == "Animator"
        }
        motions = {}
        while pending := sorted(motion_ids - motions.keys()):
            for motion_id in pending:
                reader = all_readers.get(motion_id)
                if reader is None:
                    raise ValueError(f"missing gameplay animator: {motion_id}")
                motion = self.animator_motion(
                    reader,
                    tracked_transforms.get(motion_id, set()),
                    motion_id in property_motions,
                )
                motions[motion_id] = motion
                for state in motion["states"]:
                    for behavior in state["behaviors"]:
                        if behavior["kind"] not in {
                            "ForwardTriggerToAnotherAnimator",
                            "SendTriggerAfterTime",
                            "SendTriggerToAnotherAnimator",
                            "SetBoolDuringState",
                            "SetVariableOnState",
                        }:
                            continue
                        fields = behavior["fields"]
                        name = fields.get("m_objectName") or fields.get(
                            "m_animatorName"
                        )
                        if not name:
                            continue
                        candidate = self.animator_for_object_name(
                            name, all_readers.values()
                        )
                        if candidate is not None:
                            target = f"animator-{candidate.path_id}"
                            behavior["target_animator"] = target
                            motion_ids.add(target)
        return [motions[motion_id] for motion_id in sorted(motions)]

    @staticmethod
    def animator_for_object_name(name: str, animators: Any) -> Any | None:
        candidates = []
        for animator in animators:
            transform = game_object_transform(
                animator.read().m_GameObject.deref_parse_as_object()
            )
            depth = 0
            while transform is not None:
                game_object = transform.read().m_GameObject.deref_parse_as_object()
                if game_object.m_Name == name:
                    candidates.append((depth, animator.path_id, animator))
                    break
                parent = transform.read().m_Father
                transform = parent.deref() if parent.path_id else None
                depth += 1
        candidate = min(candidates, default=None, key=lambda row: row[:2])
        return candidate[2] if candidate is not None else None

    def extract(self, name: str) -> dict[str, Any]:
        scene = asset(self.environment, f"level{self.scenes[name]}")
        managers = self.grid_managers(scene)
        barrier_layers = {10, 11, 20, 22, 23, 26}
        grid_objects = []
        seen = set()
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            class_name = mono_name(reader)
            if class_name not in {"StaticGridLocation", "DynamicGridLocation"}:
                continue
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            if transform.path_id in seen:
                continue
            seen.add(transform.path_id)
            components = self.component_names(game_object)
            manager = self.manager_for(
                transform, managers, self.transforms.world(transform)[0]
            )
            grid_objects.append(
                self.game_object_record(
                    game_object,
                    components,
                    manager,
                    class_name == "DynamicGridLocation",
                )
            )
        for reader in scene.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            class_name = mono_name(reader)
            if class_name not in STATION_CLASSES:
                continue
            game_object = (
                reader.parse_monobehaviour_head().m_GameObject.deref_parse_as_object()
            )
            transform = game_object_transform(game_object)
            if transform.path_id in seen:
                continue
            seen.add(transform.path_id)
            components = self.component_names(game_object)
            manager = self.manager_for(
                transform, managers, self.transforms.world(transform)[0]
            )
            grid_objects.append(
                self.game_object_record(game_object, components, manager, False)
            )
        for reader in scene.objects.values():
            if reader.type.name not in {"BoxCollider", "MeshCollider"}:
                continue
            collider = reader.read()
            game_object = collider.m_GameObject.deref_parse_as_object()
            transform = game_object_transform(game_object)
            current = transform
            represented = False
            while current is not None:
                if current.path_id in seen:
                    represented = True
                    break
                parent = current.read().m_Father
                current = parent.deref() if parent.path_id else None
            if (
                represented
                or not collider.m_Enabled
                or self.motion_id(transform) is None
                or (
                    game_object.m_Layer not in barrier_layers
                    and not self.is_animated_barrier(transform)
                )
            ):
                continue
            seen.add(transform.path_id)
            components = self.component_names(game_object)
            manager = self.manager_for(
                transform, managers, self.transforms.world(transform)[0]
            )
            record = self.game_object_record(game_object, components, manager, True)
            if self.is_animated_barrier(transform):
                record["components"].append("AnimatedBarrier")
                record["components"].sort()
            grid_objects.append(record)
        occupied = {
            (
                row["grid_manager"],
                row["grid"]["x"],
                row["grid"]["y"],
                row["grid"]["z"],
            )
            for row in grid_objects
            if row["layer"] in barrier_layers and row.get("motion") is None
        }
        players = self.players(scene, managers)
        ground = self.colliders(scene, managers, {9})
        self.snap_players_to_ground(players, managers, ground)
        # Besides the explicit Walls layer, worktops and their interaction
        # blockers close gaps in the kitchen boundary (serving hatches, sinks,
        # cooker blocks). They are physical navigation obstacles even when the
        # parent object does not carry a StaticGridLocation component.
        walls = self.colliders(scene, managers, barrier_layers)
        manager_data = [
            {key: value for key, value in manager.items() if key != "transform"}
            for manager in managers
        ]
        systems = self.systems(scene, managers, grid_objects)
        walkable = self.walkable(
            managers, ground, walls, occupied, players, grid_objects
        )
        return {
            "schema": "overcooked-level-v1",
            "scene": name,
            "build_index": self.scenes[name],
            "order_capacity": self.order_capacity(scene),
            "boss_flow": self.boss_flow(scene),
            "grid_managers": manager_data,
            "walkable": walkable,
            "fall_edges": self.fall_edges(walkable, managers, ground, walls),
            "objects": sorted(grid_objects, key=lambda row: row["id"]),
            "players": players,
            "initial_plates": self.plates(scene, managers, grid_objects),
            "cooking_utensils": self.cooking_utensils(scene, managers, grid_objects),
            "fire_extinguishers": self.fire_extinguishers(scene, managers),
            "systems": systems,
            "motions": self.motions(managers, grid_objects, systems, ground, scene),
        }


class Extractor:
    def __init__(self, environment: Any) -> None:
        self.environment = environment
        self.orders: dict[str, dict[str, Any]] = {}
        self.cooking_steps: dict[str, dict[str, Any]] = {}

    def order(self, owner: Any, reference: dict[str, int]) -> str | None:
        reader = deref(owner, reference)
        if reader is None:
            return None
        class_name, data = mono(reader)
        name = data["m_Name"]
        if name in self.orders:
            return name
        if class_name not in {
            "IngredientOrderNode",
            "CompositeOrderNode",
            "CookedCompositeOrderNode",
        }:
            raise ValueError(f"{name}: unsupported order node {class_name}")
        result: dict[str, Any] = {
            "id": name,
            "kind": {
                "IngredientOrderNode": "ingredient",
                "CompositeOrderNode": "composite",
                "CookedCompositeOrderNode": "cooked_composite",
            }[class_name],
            "uid": data["m_uID"],
        }
        self.orders[name] = result
        if class_name != "IngredientOrderNode":
            result["required"] = [
                self.order(reader.assets_file, item) for item in data["m_composition"]
            ]
            result["optional"] = [
                self.order(reader.assets_file, item) for item in data["m_optional"]
            ]
        if class_name == "CookedCompositeOrderNode":
            result["cooking_step"] = self.cooking_step(
                reader.assets_file, data["m_cookingStep"]
            )
            result["cooked"] = data["m_progress"] == 1
        return name

    def cooking_step(self, owner: Any, reference: dict[str, int]) -> str:
        reader = deref(owner, reference)
        if reader is None:
            raise ValueError("cooked order has no cooking step")
        class_name, data = mono(reader)
        if class_name != "CookingStepData":
            raise ValueError(f"expected CookingStepData, got {class_name}")
        name = data["m_Name"]
        self.cooking_steps.setdefault(
            name,
            {
                "id": name,
                "uid": data["m_uID"],
                "sizzle_sound_event": data["m_sizzleSound"],
                "add_sound_event": data["m_addToSound"],
            },
        )
        return name

    def recipe_entry(self, owner: Any, entry: dict[str, Any]) -> dict[str, Any]:
        return {
            "order": self.order(owner, entry["m_order"]),
            "weight": entry["m_weight"],
            "base_points_multiplier": entry["m_basePointsMultiplier"],
            "additional_points": entry["m_additionalPoints"],
        }

    def recipe_list(
        self, owner: Any, reference: dict[str, int]
    ) -> dict[str, Any] | None:
        reader = deref(owner, reference)
        if reader is None:
            return None
        class_name, data = mono(reader)
        if class_name != "RecipeList":
            raise ValueError(f"expected RecipeList, got {class_name}")
        return {
            "id": data["m_Name"],
            "entries": [
                self.recipe_entry(reader.assets_file, entry)
                for entry in data["m_recipes"]
                if entry["m_order"]["m_PathID"]
            ],
        }

    def round(self, owner: Any, data: dict[str, Any]) -> dict[str, Any]:
        result = {
            "duration_seconds": data["m_roundTimer"],
            "recipes": self.recipe_list(owner, data["m_recipes"]),
        }
        if "m_manualOrder" in data:
            result["manual_order"] = [
                self.recipe_entry(owner, entry)
                for entry in data["m_manualOrder"]
                if entry["m_order"]["m_PathID"]
            ]
        return result

    def config(self, owner: Any, reference: dict[str, int]) -> dict[str, Any]:
        reader = deref(owner, reference)
        if reader is None:
            raise ValueError("campaign variant has no level config")
        class_name, data = mono(reader)
        result: dict[str, Any] = {
            "id": data["m_Name"],
            "kind": class_name,
            "plate_return_seconds": data["m_plateReturnTime"],
            "show_icon_prompts": bool(data["m_showIconPrompts"]),
            "disable_dynamic_parenting": bool(data["m_disableDynamicParenting"]),
            "grid_selection": bool(data["m_gridSelection"]),
        }
        hazard = deref(reader.assets_file, data["m_hazardInfo"])
        if hazard is not None:
            _, hazard_data = mono(hazard)
            fire = deref(hazard.assets_file, hazard_data["FireConfigData"])
            if fire is not None:
                _, fire_data = mono(fire)
                result["fire"] = {
                    "recovery_seconds": fire_data["FireRecoveryTime"],
                    "flammability_seconds": fire_data["FlammabilityTime"],
                    "cooldown_seconds": fire_data["CooldownTime"],
                    "encouragement_suppressed_seconds": fire_data[
                        "EncouragementSupressedTime"
                    ],
                    "cooldown_suppressed_seconds": fire_data["CooldownSupressedTime"],
                }
        if class_name in {"CampaignLevelConfig", "ScriptedCampaignLevelConfig"}:
            result.update(
                {
                    "order_lifetime_seconds": data["m_orderLifetime"],
                    "seconds_between_orders": data["m_timeBetweenOrders"],
                    "rounds": [
                        self.round(reader.assets_file, row) for row in data["m_rounds"]
                    ],
                }
            )
        elif class_name == "BossCampaignLevelConfig":
            boss = data["m_data"]
            result.update(
                {
                    "order_lifetime_seconds": data["m_orderLifetime"],
                    "seconds_between_orders": data["m_timeBetweenOrders"],
                    "duration_seconds": boss["m_roundTimer"],
                    "phases": [
                        [
                            self.recipe_entry(reader.assets_file, entry)
                            for entry in phase["RecipeOrder"]
                        ]
                        for phase in boss["Phases"]
                    ],
                }
            )
        elif class_name == "SinglePlayerLevelConfig":
            result.update(
                {
                    "duration_seconds": data["TimeLimit"],
                    "manual_order": [
                        self.recipe_entry(reader.assets_file, entry)
                        for entry in data["RecipeOrder"]
                    ],
                    "time_star_boundaries_seconds": [
                        data["m_timeForOneStar"],
                        data["m_timeForTwoStars"],
                        data["m_timeForThreeStars"],
                    ],
                }
            )
        else:
            raise ValueError(f"unsupported main campaign config {class_name}")
        return result

    def scene_directory(self) -> tuple[Any, dict[str, Any], list[int], set[int]]:
        world = asset(self.environment, "level45")
        flow_reader = None
        kitchen_portals: set[int] = set()
        all_portals: set[int] = set()
        for reader in world.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            class_name = mono_name(reader)
            if class_name == "WorldMapFlowController":
                _, data = mono(reader)
                flow_reader = reader
                directory_reader = deref(world, data["m_sceneDirectory"])
            elif class_name in {"LevelPortalMapNode", "MiniLevelPortalMapNode"}:
                _, data = mono(reader)
                all_portals.add(data["m_levelIndex"])
                if class_name == "LevelPortalMapNode":
                    kitchen_portals.add(data["m_levelIndex"])
        if flow_reader is None or directory_reader is None:
            raise ValueError("WorldMap_25 has no scene directory")
        class_name, directory = mono(directory_reader)
        if class_name != "SceneDirectoryData":
            raise ValueError(f"world map directory has type {class_name}")

        sequence = [39]
        while True:
            next_entries = [
                index
                for index in all_portals
                if index not in sequence
                and sequence[-1]
                in directory["Scenes"][index]["PreviousEntriesToUnlock"]
            ]
            if not next_entries:
                break
            if len(next_entries) != 1:
                raise ValueError(
                    f"campaign chain branches after {sequence[-1]}: {next_entries}"
                )
            sequence.append(next_entries[0])
        if set(sequence) != all_portals:
            missing = sorted(all_portals - set(sequence))
            if missing != [40]:
                raise ValueError(f"campaign chain does not reach portals {missing}")
        return directory_reader, directory, sequence, kitchen_portals

    def campaign(self) -> dict[str, Any]:
        owner, directory, sequence, kitchen_portals = self.scene_directory()
        levels = []
        for sequence_index, directory_index in enumerate(sequence):
            if directory_index not in kitchen_portals:
                continue
            entry = directory["Scenes"][directory_index]
            variants = []
            for variant in entry["SceneVarients"]:
                variants.append(
                    {
                        "players": variant["PlayerCount"],
                        "scene": variant["SceneName"],
                        "score_star_boundaries": [
                            variant["OneStarScore"],
                            variant["TwoStarScore"],
                            variant["ThreeStarScore"],
                        ],
                        "config": self.config(
                            owner.assets_file, variant["LevelConfig"]
                        ),
                    }
                )
            levels.append(
                {
                    "number": len(levels) + 1,
                    "directory_index": directory_index,
                    "chain_position": sequence_index,
                    "label": entry["Label"],
                    "star_cost": entry["StarCost"],
                    "previous_directory_indices": entry["PreviousEntriesToUnlock"],
                    "variants": variants,
                }
            )
        if len(levels) != 30:
            raise ValueError(f"expected 30 main kitchens, extracted {len(levels)}")
        return {
            "schema": "overcooked-campaign-v1",
            "source": {
                "title": "Overcooked",
                "steam_app_id": APP_ID,
                "depot_manifest": DEPOT_MANIFEST,
                "unity_version": UNITY_VERSION,
            },
            "scoring": self.game_config(),
            "main_campaign": levels,
            "orders": sorted(self.orders.values(), key=lambda row: row["id"]),
            "cooking_steps": sorted(
                self.cooking_steps.values(), key=lambda row: row["id"]
            ),
        }

    def game_config(self) -> dict[str, Any]:
        resources = asset(self.environment, "resources.assets")
        for reader in resources.objects.values():
            if reader.type.name != "MonoBehaviour":
                continue
            if mono_name(reader) == "GameConfig":
                _, data = mono(reader)
                chop_impact_seconds = self.chop_impact_seconds()
                return {
                    "default_delivery_points": data["DefaultDeliveryAward"],
                    "expired_order_penalty": data["RecipeTimeOutPointLoss"],
                    "single_player_chops_per_stage": data[
                        "SingleplayerChopTimeMultiplier"
                    ],
                    "chop_impact_seconds": chop_impact_seconds,
                    "tip_boundaries": [
                        {
                            "remaining_fraction_exclusive_min": row[
                                "PercentageTimeRemaining"
                            ],
                            "points": row["ScoreValue"],
                        }
                        for row in data["TipBoundaries"]
                    ],
                }
        raise ValueError("GameConfig not found")

    def chop_impact_seconds(self) -> float:
        matches = []
        for serialized in self.environment.assets:
            for reader in serialized.objects.values():
                if reader.type.name != "AnimationClip":
                    continue
                data = reader.read_typetree()
                if data["m_Name"] != "New_Chef@Chop":
                    continue
                matches.extend(
                    event["time"]
                    for event in data["m_Events"]
                    if event["functionName"] == "OnTrigger"
                    and event["data"] == "Impact"
                )
        if len(matches) != 1:
            raise ValueError(f"expected one chef chop impact, found {matches}")
        return matches[0]


def build_scenes(environment: Any) -> dict[str, int]:
    managers = asset(environment, "globalgamemanagers")
    for reader in managers.objects.values():
        if reader.type.name != "BuildSettings":
            continue
        return {
            path.replace("\\", "/").rsplit("/", 1)[-1].removesuffix(".unity"): index
            for index, path in enumerate(reader.read_typetree()["scenes"])
        }
    raise ValueError("BuildSettings not found")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--game-root", type=Path)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "data/overcooked-1",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify that the committed extraction is byte-for-byte current",
    )
    args = parser.parse_args()
    data_root = locate_data_root(args.game_root)
    verify_source(data_root)

    environment = UnityPy.load(str(data_root))
    generator = TypeTreeGenerator(UNITY_VERSION)
    generator.load_local_dll_folder(str(data_root / "Managed"))
    environment.typetree_generator = generator

    campaign = Extractor(environment).campaign()
    campaign_data = json_bytes(campaign)
    if args.check:
        if (args.output / "campaign.json").read_bytes() != campaign_data:
            raise SystemExit("campaign.json is not current")
    else:
        args.output.mkdir(parents=True, exist_ok=True)
        (args.output / "campaign.json").write_bytes(campaign_data)
    scene_names = sorted(
        {
            variant["scene"]
            for level in campaign["main_campaign"]
            for variant in level["variants"]
        }
    )
    scene_extractor = SceneExtractor(environment, build_scenes(environment))
    levels_directory = args.output / "levels"
    if not args.check:
        levels_directory.mkdir(parents=True, exist_ok=True)
    level_inventory = []
    expected_level_files = {f"{name}.json" for name in scene_names}
    actual_level_files = {path.name for path in levels_directory.glob("*.json")}
    if args.check and actual_level_files != expected_level_files:
        raise SystemExit("committed level file inventory is not current")
    if not args.check:
        for stale in levels_directory.glob("*.json"):
            if stale.name not in expected_level_files:
                stale.unlink()
    for name in scene_names:
        level = scene_extractor.extract(name)
        level_data = json_bytes(level)
        path = levels_directory / f"{name}.json"
        if args.check:
            if path.read_bytes() != level_data:
                raise SystemExit(f"{path.name} is not current")
        else:
            path.write_bytes(level_data)
        level_inventory.append(
            {
                "scene": name,
                "file": f"levels/{path.name}",
                "sha256": hashlib.sha256(level_data).hexdigest(),
                "grid_managers": len(level["grid_managers"]),
                "walkable_cells": len(level["walkable"]),
                "grid_objects": len(level["objects"]),
                "players": len(level["players"]),
                "initial_plates": len(level["initial_plates"]),
                "cooking_utensils": len(level["cooking_utensils"]),
                "systems": len(level["systems"]),
            }
        )
    manifest = {
        "schema": "overcooked-gameplay-import-v1",
        "source": campaign["source"],
        "source_sha256": SOURCE_HASHES,
        "inventory": {
            "main_campaign_kitchens": len(campaign["main_campaign"]),
            "player_variants": sum(
                len(level["variants"]) for level in campaign["main_campaign"]
            ),
            "level_scenes": len(level_inventory),
            "walkable_cells": sum(row["walkable_cells"] for row in level_inventory),
            "grid_objects": sum(row["grid_objects"] for row in level_inventory),
            "dynamic_systems": sum(row["systems"] for row in level_inventory),
            "order_nodes": len(campaign["orders"]),
            "cooking_steps": len(campaign["cooking_steps"]),
        },
        "levels": level_inventory,
        "campaign_sha256": hashlib.sha256(campaign_data).hexdigest(),
        "excluded": [
            "audio",
            "textures",
            "models",
            "presentation_only_animation_channels",
            "localization",
            "steam_account_data",
            "user_saves",
        ],
    }
    manifest_data = json_bytes(manifest)
    if args.check:
        if (args.output / "manifest.json").read_bytes() != manifest_data:
            raise SystemExit("manifest.json is not current")
    else:
        (args.output / "manifest.json").write_bytes(manifest_data)
    print(
        f"{'verified' if args.check else 'wrote'} "
        f"{manifest['inventory']['main_campaign_kitchens']} kitchens, "
        f"{manifest['inventory']['player_variants']} variants, "
        f"{manifest['inventory']['level_scenes']} scene layouts, "
        f"{manifest['inventory']['order_nodes']} order nodes"
    )


if __name__ == "__main__":
    main()
